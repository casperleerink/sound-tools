import { boolean, choice, Fragment, h, list, memo, number, Page, Rect, Space, text, tool, Transport, untrack, type Accessor, type Connection, type Context, type Declaration, type Inputs, type Turn, type ValuesOf } from "./sdk";
import { bus } from "./bus";
import { ClipEditor, ROW as NOTE_ROW, type EditorMemory } from "./clip-editor";
import { BEAT } from "./piece-form";
import { BarStrip, Rows } from "./piece-bars";
import { edit, isLaidOut, type Chosen, type Copied } from "./piece-edits";
import { pieceActions } from "./piece-keys";
import { Mixer } from "./piece-mixer";
import { beatRuns, clamp, colourOf, editedPart, editPart, formOf, LOW_ROW, openOf, openSectionOf, pageOf, showSection, ROW, SECTION_HEAD, STRIP, titleOf } from "./piece-page";
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
  /** The chosen bars of the open section: whole, or the music of some instruments in them. */
  chosen: Chosen | null;
  /** The section that plays, by its id, while the piece plays: the playhead opens it as it gets there. */
  playing: string | null;
  /** Whether the page follows the playhead: not after the composer chose something as it played, until the next play. */
  follows: boolean;
  /** The bar of the open section a key chose a note in, which the note editor then shows: none once bars are chosen. */
  noteBar: number | null;
  /** What an edit took that the composer did not ask it to take, said in the section head, with the sections as that edit left them: it shows only while they are so, so not after an undo. */
  notice: { text: string; form: string } | null;
  /** The number of the section head that is typed. */
  typing: "length" | "beats" | "note" | "tempo" | null;
  /** The section whose name is typed, in the list or in the head. */
  naming: { id: string; in: "list" | "head" } | null;
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
    "A piece in sections of bars, as a score has them. `sections` is the form, in order: each has an `id` (`sec-` and a name), a `name` and its `bars` as runs of alike bars, so every bar has its own time signature and tempo: `{ \"id\": \"sec-theme\", \"name\": \"Theme\", \"bars\": [{ \"bars\": 4, \"signature\": \"3/4\", \"bpm\": 96 }, { \"bars\": 1, \"signature\": \"4/4\", \"bpm\": 96 }] }` is five bars, the last of 4/4. A bar's `bpm` counts quarter notes; the page says and takes a tempo in the bar's own beat, a dotted quarter in 6/8, 9/8 and 12/8. Each instrument is a `track` child; its part in a section is its `timeline` child named by the section's id, in `state/piece/<instrument>/<section id>.json`, its notes in ticks (960 a beat) from the start of that section, none past its end. An instrument with no music in a section has no part there. The piece lays the sections one after the other and sets, from its own `sections`, each part's `start` and `length` and the clock's tempo map and section names: change bars, time signatures and tempos in the piece's record, never in `project.json` or a part's `start` and `length`. After a change by hand it lays them out again by itself. Each instrument and each return `ret-*` plays into the fader `master`, then its limiter `limit` unless `limit_off`; `order` is the order of the instruments, `solo` names those that play alone; each of `sidechains` keys an effect, by its path such as `t00/chain/comp`, with what an instrument `from` plays at `tap`: into its effects, out of them or out of its fader. The page shows one section at a time, its bars as wide as they are long. The column of sections: a click (or enter) opens one and, while the piece is stopped, puts the playhead there, a double click or shift-R names it, a right-click, or the … beside its name in the head, renames, duplicates, moves, joins or deletes it, a drag moves it, Add section adds one at the end and shift-S one after the open section; a green dot marks the one that plays, and the playhead opens each section it reaches, until the composer chooses something as it plays: a choice then moves nothing that is heard, a click on the section that plays follows it again, and after a pause it plays from what was chosen. The head of the open section: its length in bars, and the time signature and tempo of the chosen bars or of all (a drag up or down, a click or enter to type, `6/8` typed whole; one undo step; a dash where the bars differ, which only typing then changes; a time signature holds still while either of its numbers differs; T types the tempo, shift-T the time signature, shift-L the length; notes a shorter time signature or length removes are said in place of where the section is, until the next edit, choice or section), a click on the caption of the chosen bars lets go of them, and Loop (L), which loops the chosen bars or the section as it plays, a line at the foot of the bar strip. The bar strip chooses whole bars: a click, shift-click or a drag; its plus adds a bar at the end, shift-B one after them, alt-shift-B one before, shift-S starts a section at the first. A row per instrument draws the notes of its part: a click chooses that bar of that instrument, shift-click or a drag a passage of bars and instruments; a double click or enter opens its notes below, on the chosen bars. On what is chosen: cmd-C, cmd-X, cmd-V and cmd-D copy, cut, paste and repeat it (whole bars paste and repeat as new bars put in, music over the bars from the first chosen one, the section getting bars at its end where it has too few), delete removes whole bars or clears music, the arrows choose the bar before or after, across into the section before or the next, or the instrument above or below (up from the first instrument chooses the whole bars), shift with them takes more in, inside the section, with none chosen left or right chooses the bar that plays, cmd-shift-right takes in the bars up to the last, so a tempo goes on from here, cmd-A chooses all bars; a right-click, or the … beside the chosen bars in the head, shows these. Alt-shift-left and alt-shift-right open the section before and the next, alt-shift-up and alt-shift-down move it. Below the rows shows one of: an instrument's or a return's devices (a click on its head; a double click names it, a right-click renames, colours or deletes it, a drag moves it; with nothing chosen enter renames it, delete deletes it, up and down choose another, alt-up and alt-down move it), the master's (its row), the mixer (the toggle on its row) or the notes of a part. In the note editor N adds a note, alt-left and alt-right choose the note before and the next as they sound, from the playhead on, and the editor shows its bar, shift-left and shift-right make the chosen notes shorter and longer, its Grid is the note value notes snap to, triplets too, Q quantizes, the arrows move the chosen notes, alt-up and alt-down change their velocity, cmd-A, cmd-C, cmd-X, cmd-V and cmd-D work on notes while some are chosen, shift-enter makes it large; it follows the open section, empty where its instrument has no part yet, a note ends where its section does, and its first note makes the part. A click under the rows lets go of the chosen bars. Escape lets go of chosen notes, then the chosen effect, then chosen bars, then closes the editor, then what shows below. Space plays and pauses. The keys named here are the first keys of its `actions`, in `piece-keys.ts`.",
  state: fields,
  children: parts,
  memory: (): Memory => ({
    open: null, chosen: null, playing: null, follows: true, noteBar: null, notice: null, typing: null, naming: null, copied: null, below: null, track: null, hovered: null, renaming: null, returnsOpen: false, names: {}, effect: null, opened: false, editor: EDITOR, editorLarge: false,
    notes: new Set(), snap: BEAT / 4, lane: "velocity", length: BEAT / 2, copiedNotes: null, strength: 1, swing: 0, humanized: 0,
  }),
  actions: pieceActions,
  tick: (tool) => {
    const { memory, clock, state } = tool;
    const places = Object.entries(formOf(state).places);
    const playing = clock.playing ? (places.find(([, at]) => clock.tick >= at.start && clock.tick < at.start + at.length)?.[0] ?? null) : null;
    if (!clock.playing && !memory.follows) {
      memory.follows = true;
      // What was chosen as it played is where it plays from next, as a choice puts the playhead while it is stopped. Not after a stop, which went back to the start.
      const open = openOf(tool);
      if (open && clock.tick !== 0) clock.jump(open.place.start + (open.starts[Math.min(memory.chosen?.from ?? 0, open.bars.length - 1)] ?? 0));
    }
    if (playing === memory.playing) return;
    memory.playing = playing;
    if (playing !== null && memory.follows && playing !== openSectionOf(tool)?.id) showSection(tool, playing);
  },
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
      untrack(() => edit({ update: tool.update, clock }, "Lay out the piece", () => {}));
    });
    // A repeat stops when the bars it covers, or those before it, move: it would repeat other music.
    let repeated = "";
    memo(() => {
      const repeat = clock.repeat;
      const now = repeat ? `${repeat.from}-${repeat.to}:${state.sections.map((section) => section.id)}:${pieceStarts().filter((tick) => tick <= repeat.to)}` : "";
      if (repeated.split(":")[0] === now.split(":")[0] && repeated !== now) untrack(() => clock.setRepeat(null));
      repeated = now;
    });

    // The editor at its largest leaves the section head, the bar strip, one row, the returns and the master over it.
    const largest = () => Math.max(tool.size.height - DIVIDER - SECTION_HEAD - 1 - STRIP - 1 - ROW - page.returnsHeight() - LOW_ROW, SHORTEST);
    const editorHeight = () => (memory.editorLarge ? largest() : clamp(memory.editor, SHORTEST, largest()));
    // The bars the editor opens on: the chosen ones with a bar each side, which it keeps when they are let go, and in a section with none chosen yet its first eight.
    let kept: { id: string | null; from: number; to: number } = { id: null, from: 0, to: 8 };
    const shownBars = memo(() => {
      const [id, chosen, note] = [page.openId(), memory.chosen, memory.noteBar];
      // A note a key chose outside them takes the editor to its bar.
      if (note !== null && (kept.id !== id || note < kept.from || note >= kept.to)) kept = { id, from: Math.max(note - 1, 0), to: note + 2 };
      else if (note === null && chosen) kept = { id, from: Math.max(chosen.from - 1, 0), to: chosen.to + 2 };
      else if (kept.id !== id) kept = { id, from: 0, to: 8 };
      return `${kept.from}-${kept.to}`;
    });
    /** The ticks of the open section those bars are, as it is now. */
    const shownTicks = (bars: string): readonly [number, number] => {
      const starts = untrack(page.starts);
      const [from = 0, to = 8] = bars.split("-").map(Number);
      const last = starts.length - 1;
      return [starts[clamp(from, 0, last)] ?? 0, starts[clamp(to, 0, last)] ?? 0];
    };
    const editorOf = (name: string, id: string, shows: readonly [number, number]) => {
      const row = () => rows().find((each) => each.name === name);
      return (
        <ClipEditor
          clip={() => editedPart(tool)}
          bars={() => pieceStarts().map((tick) => tick - (place()?.start ?? 0))}
          name={() => (row() ? titleOf(row()!) : name)}
          title={() => ""}
          colour={() => (row() ? colourOf(row()!) : "blue")}
          id={`${name}/${id}/${shows[0]}-${shows[1]}`}
          shows={shows}
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
            // Made again when other bars are chosen, so it shows them.
            shown?.kind === "editor" && open !== null ? editorOf(shown.track, open, shownTicks(shownBars()))
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
