import { boolean, choice, Fragment, h, list, memo, number, Page, Rect, Space, text, tool, Transport, untrack, type Accessor, type Connection, type Context, type Declaration, type Inputs, type Turn, type ValuesOf } from "./sdk";
import { bus } from "./bus";
import { ClipEditor, ROW as NOTE_ROW, type EditorMemory } from "./clip-editor";
import { BEAT } from "./piece-form";
import { BarStrip, Rows } from "./piece-bars";
import { edit, isLaidOut, type Chosen, type Copied } from "./piece-edits";
import { pieceActions } from "./piece-keys";
import { Mixer } from "./piece-mixer";
import { beatRuns, clamp, colourOf, editedPart, editPart, LOW_ROW, pageOf, ROW, SECTION_HEAD, STRIP, titleOf } from "./piece-page";
import { MasterPanel, Rack } from "./piece-panel";
import { SectionHead, SectionsList } from "./piece-sections";
import { Heads } from "./piece-tracks";
import { track } from "./track";

// A piece: sections of bars, instruments that play a part in each, a mixer strip per instrument,
// the devices of the chosen instrument, and a note editor for one part.
export const parts = { master: "fader", limit: "limiter", quiet: "lane", "ret-*": bus, "*": track } as const;
const bars = { bars: number({ min: 1, max: 999, default: 1, integer: true }), signature: text({ default: "4/4" }), bpm: number({ min: 10, max: 1000, default: 120 }) };
const fields = {
  sections: list({ id: text(), name: text(), bars: list(bars, { max: 512 }) }, { max: 256 }),
  order: list(text(), { max: 128 }),
  solo: list(text(), { max: 64 }),
  sidechains: list({ effect: text(), from: text(), tap: choice({ options: ["pre_fx", "post_fx", "post_fader"], default: "post_fader" }) }, { max: 64 }),
  limit_off: boolean({ default: false }),
};
/** What the piece's code gets: its view, the parts of its page and its actions. */
export type Piece = Context<ValuesOf<typeof fields>, Inputs, Memory, typeof parts>;
/** How tall the note editor opens, and the shortest a drag makes it. */
const [EDITOR, SHORTEST] = [352, 184];
/** The line over the editor that drags, with the room around it the pointer finds. */
const DIVIDER = 5;

/** What shows under the rows, one at a time: an instrument's or a return's panel, the master's, the notes of an instrument's part in the open section, or the mixer. */
type Below = { kind: "track"; name: string } | { kind: "master" } | { kind: "editor"; track: string } | { kind: "mixer" } | null;

type Memory = {
  /** The section that shows, by its id: the first while none is. */
  open: string | null;
  /** The chosen bars of the open section. */
  chosen: Chosen | null;
  copied: Copied | null;
  below: Below;
  /** The instrument the keyboard plays, as a click on a strip's name chooses it beside the mixer. */
  track: string | null;
  /** The instrument whose head the pointer is over, which shows all its toggles. */
  hovered: string | null;
  renaming: string | null;
  /** Whether the returns show a row each, or fold to one. */
  returnsOpen: boolean;
  /** What plays each instrument, by `name/synth`, as its card's title says: for its head. */
  names: Record<string, string>;
  /** The chosen effect of a panel, by its name in the chain, which the keys move. */
  effect: { track: string; name: string } | null;
  /** Whether it showed the panel of its first instrument, as it does once, when it opens. */
  opened: boolean;
  /** How tall the note editor is, in points, and whether it is large: all the window but one row. */
  editor: number;
  editorLarge: boolean;
} & EditorMemory;

export const piece = tool({
  name: "piece",
  title: "Piece",
  when: "You change the piece's page: its sections and bars, its instruments, mixer strips or note editor",
  doc:
    "A piece in sections of bars, as a score has them. `sections` is the form, in order: each has an `id` (`sec-intro`), a `name` and its `bars` as runs of alike bars, `{ bars, signature, bpm }`, so every bar has its own time signature and tempo; the piece lays them out one after the other and sets the clock's tempo map and section names from them. Each instrument, a `track` child, plays a part in a section as its `timeline` child named by the section's id, its notes in ticks from the start of that section; an instrument with no music in a section has no part there. A part's `start` and `length` are where its section is and how long: the piece sets them, never write them by hand; after a change to `sections` it lays the parts and the clock out again by itself. Each instrument and each return `ret-*` plays into the fader `master`, then its limiter `limit` unless `limit_off`; `order` is the order of the instruments, `solo` names those that play alone. Each of `sidechains` keys an effect, by its path such as `t00/chain/comp`, with what an instrument `from` plays at `tap`: into its effects, out of them or out of its fader. The page shows one section at a time, chosen in the column at its left, with a row per instrument; under them shows one of: an instrument's or a return's devices (a click on its head; a double click names it, a right-click renames, colours or deletes it, a drag onto another head moves it), the master's (its row), the mixer (the toggle on the master's row) or the notes of an instrument's part in the open section. A piece opens on its first section with the panel of its first instrument. In a panel an effect is dragged to another place in its chain, or moved a step by the menu of its title or, chosen, by alt-left and alt-right. Escape lets go of what is chosen, then closes what shows below; on a panel, enter renames, delete asks and deletes, up and down choose another instrument and alt-up and alt-down move it. In the note editor N adds a note, Q quantizes, the arrows move the chosen notes (alt-up and alt-down change their velocity), cmd-A, cmd-C, cmd-X, cmd-V and cmd-D choose, copy, cut, paste and duplicate them, delete removes them, and shift-enter makes it large; its notes go into the part, which it makes on the first note when the instrument has none in that section. Space plays and pauses. The keys named here are the first keys of its `actions`, in `piece-keys.ts`.",
  state: fields,
  children: parts,
  memory: (): Memory => ({
    open: null, chosen: null, copied: null, below: null, track: null, hovered: null, renaming: null, returnsOpen: false, names: {}, effect: null, opened: false, editor: EDITOR, editorLarge: false,
    notes: new Set(), snap: BEAT / 4, lane: "velocity", length: BEAT / 2, copiedNotes: null, strength: 1, swing: 0, humanized: 0,
  }),
  actions: pieceActions,
  behaviour: ({ state, children }): Declaration<typeof parts> => {
    const tracks = children.filter((child) => child.tool === "track");
    const returns = children.filter((child) => child.tool === "bus");
    const soloed = state.solo.filter((name) => tracks.some((each) => each.name === name));
    const connections: Connection<typeof parts>[] = [
      ...[...tracks, ...returns].map((each) => ({ from: { child: each.name }, to: { child: "master" } })),
      ...(state.limit_off
        ? [{ from: { child: "master" }, to: { port: "audio" } } as const]
        : [{ from: { child: "master" }, to: { child: "limit" } } as const, { from: { child: "limit" }, to: { port: "audio" } } as const]),
    ];
    // What the piece hears, the keyboard while nothing is chosen, plays its first instrument.
    const rank = (name: string) => (state.order.includes(name) ? state.order.indexOf(name) : state.order.length);
    const first = [...tracks].sort((a, b) => rank(a.name) - rank(b.name) || (a.name < b.name ? -1 : 1))[0];
    if (first) connections.push({ from: { notes: "notes" }, to: { child: first.name, notes: "notes" } });
    // A solo mutes every other instrument's fader, gliding; a return always plays.
    const soloedOut = (name: string) => soloed.length > 0 && !soloed.includes(name);
    for (const each of tracks) {
      if (soloedOut(each.name)) connections.push({ from: { child: "quiet", number: "value" }, to: { child: `${each.name}/mix` as const, number: "mute" } });
    }
    // A send of an instrument plays into its return: after its fader, or before it for a return that says so. A muted or soloed-out one sends nothing either way.
    for (const each of tracks) {
      const mix = each.children.find((child) => child.name === "mix");
      const silent = soloedOut(each.name) || (mix?.tool === "fader" && mix.state.mute === true);
      for (const back of returns) {
        if (back.tool !== "bus" || !each.children.some((child) => child.name === `send-${back.name}`)) continue;
        const from = back.state.pre_fader ? "chain" : "mix";
        const send = `${each.name}/send-${back.name}` as const;
        connections.push({ from: { child: `${each.name}/${from}` as const }, to: { child: send } });
        connections.push({ from: { child: send }, to: { child: back.name } });
        if (back.state.pre_fader && silent) connections.push({ from: { child: "quiet", number: "value" }, to: { child: send, number: "mute" } });
      }
    }
    // A sidechain keys an effect with an instrument: what plays into its effects, what comes out of them, or what comes out of its fader.
    for (const { effect, from, tap } of state.sidechains) {
      if (!tracks.some((each) => each.name === from)) continue;
      const key = tap === "pre_fx" ? "synth" : tap === "post_fx" ? "chain" : "mix";
      connections.push({ from: { child: `${from}/${key}` as `${string}/${string}` }, to: { child: effect as `${string}/${string}`, port: "sidechain" } });
    }
    return { connections };
  },
  view: (tool) => {
    const { memory, clock, play, children, state } = tool;
    const page = pageOf(tool);
    const { rows, strips, place, pieceStarts } = page;
    // A piece opens on its first section and its first instrument, chosen, with its panel: a click on an instrument shows the same.
    if (!untrack(() => memory.opened)) {
      memory.opened = true;
      const first = untrack(rows)[0];
      if (first) untrack(() => page.openPanel(first.name));
    }
    // A record changed by hand, such as by the agent, is laid out again: once for what it finds, so a clock that refuses the tempo map cannot make it try again and again.
    let tried = "";
    memo(() => {
      if (isLaidOut(tool)) return;
      const found = JSON.stringify([state.sections, children, clock.tempo_map, clock.sections]);
      if (found === tried) return;
      tried = found;
      untrack(() => edit(tool, "Lay out the piece", () => {}));
    });

    // The editor at its largest leaves the section head, the bar strip, one row, the returns and the master over it.
    const largest = () => Math.max(tool.size.height - DIVIDER - SECTION_HEAD - 1 - STRIP - 1 - ROW - page.returnsHeight() - LOW_ROW, SHORTEST);
    const editorHeight = () => (memory.editorLarge ? largest() : clamp(memory.editor, SHORTEST, largest()));
    const editorOf = (name: string, id: string) => {
      const row = () => rows().find((each) => each.name === name);
      return (
        <ClipEditor
          clip={() => editedPart(tool)}
          bars={() => pieceStarts().map((tick) => tick - (place()?.start ?? 0))}
          name={() => (row() ? titleOf(row()!) : name)}
          title={() => ""}
          colour={() => (row() ? colourOf(row()!) : "blue")}
          id={`${name}/${id}`}
          memory={memory}
          edit={(label, change) => editPart(tool, name, id, label, change)}
          preview={(pitch) => play(pitch, { child: name, seconds: 0.3 })}
          seek={(tick) => clock.jump((place()?.start ?? 0) + tick)}
          onClose={() => (memory.below = { kind: "track", name })}
          height={editorHeight}
          large={() => memory.editorLarge}
          onLarge={() => (memory.editorLarge = !memory.editorLarge)}
          beats={() => beatRuns(clock.tempo_map.time_signatures)}
          openMenu={page.openMenu}
          hearsControllers={() => row()?.children.some((child) => child.name === "synth" && String(child.tool) === "plugin") ?? false}
        />
      );
    };
    const Hairline = () => <div style={{ height: 1, background: "border", keep: true }} />;
    const Rule = () => <div style={{ width: 1, background: "border", keep: true }} />;

    return (
      <Page flush header={<Transport computerKeys tempo={false} />}>
        <div style={{ direction: "row", grow: true }}>
          <SectionsList page={page} />
          <Rule />
          <div style={{ grow: true }}>
            <SectionHead page={page} />
            <Hairline />
            <div style={{ direction: "row", grow: true }}>
              <Heads page={page} />
              <Rule />
              <div style={{ grow: true }}>
                <BarStrip page={page} />
                <Rows page={page} />
              </div>
            </div>
          </div>
        </div>
        {() => {
          const shown = memory.below;
          const open = shown?.kind === "editor" ? page.openId() : null;
          const below =
            shown?.kind === "editor" && open !== null ? editorOf(shown.track, open)
            : shown?.kind === "mixer" ? <Mixer page={page} />
            : shown?.kind === "master" ? <MasterPanel page={page} />
            : shown?.kind === "track" && strips().some((each) => each.name === shown.name) ? <Rack page={page} name={shown.name} />
            : null;
          const resize = (height: number) => {
            memory.editor = clamp(height, SHORTEST, largest());
            memory.editorLarge = false;
          };
          return below === null ? null : (
            <>
              {shown?.kind === "editor" ? <Divider height={editorHeight} resize={resize} /> : <Hairline />}
              {below}
            </>
          );
        }}
      </Page>
    );
  },
});

/**
 * The line over the note editor: a drag up makes the editor taller, a row of notes at a time,
 * and a double click puts it back as it opens. A tab stop, which the arrows turn.
 */
const Divider = ({ height, resize }: { height: Accessor<number>; resize: (height: number) => void }) => {
  let pressed = EDITOR;
  const onTurn = (turn: Turn) => {
    if ("reset" in turn) resize(EDITOR);
    else if ("moved" in turn) resize((turn.held ? pressed : height()) + turn.moved * NOTE_ROW);
  };
  return (
    <Space key="editor-divider" height={DIVIDER} cursor="ns-resize" onPress={() => (pressed = height())} drag={{ y: { onTurn, step: 1, per: NOTE_ROW, default: EDITOR } }}>
      <Rect x={0} y={(DIVIDER - 1) / 2} width={1e5} height={1} color="border" />
    </Space>
  );
};
