import { boolean, choice, Fragment, h, list, memo, number, Page, Rect, Space, text, tool, Transport, untrack, type Accessor, type Connection, type Context, type Declaration, type Inputs, type Turn, type ValuesOf } from "./sdk";
import { bus } from "./bus";
import { ClipEditor, ROW as NOTE_ROW, type EditorMemory } from "./clip-editor";
import { BEAT } from "./piece-form";
import { Marks, Rows } from "./piece-bars";
import { edit, isLaidOut, type Chosen, type Copied } from "./piece-edits";
import { pieceActions } from "./piece-keys";
import { Mixer } from "./piece-mixer";
import { barsOfPiece, barsOfSection, beatRuns, clamp, colourOf, editedPart, editPart, LOW_ROW, MARKS, pageOf, partSectionOf, ROW, titleOf } from "./piece-page";
import { MasterPanel, Rack } from "./piece-panel";
import { Heads } from "./piece-tracks";
import { track } from "./track";

// A piece: sections of bars, instruments that play a part in each, a mixer strip per instrument,
// the devices of the chosen instrument, and a note editor for one part.
export const parts = { master: "fader", limit: "limiter", quiet: "lane", "ret-*": bus, "*": track } as const;
const bars = {
  bars: number({ min: 1, max: 999, default: 1, integer: true }), signature: text({ default: "4/4" }), bpm: number({ min: 10, max: 1000, default: 120 }),
  to: number({ min: 0, max: 1000, default: 0 }), fermata: number({ min: 0, max: 64, default: 0, integer: true }), hold: number({ min: 0, max: 64, default: 0, unit: "plain" }),
};
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

/** What shows under the rows, one at a time: an instrument's or a return's panel, the master's, the notes of an instrument's part, or the mixer. */
type Below = { kind: "track"; name: string } | { kind: "master" } | { kind: "editor"; track: string } | { kind: "mixer" } | null;

type Memory = {
  /** The chosen bars of the piece: whole, or the music of some instruments in them. */
  chosen: Chosen | null;
  /** The section whose part the note editor shows, by its id: that of the first chosen bar, the first while none is. */
  part: string | null;
  /** Whether the page turns with the playhead: not after the composer chose something as it played, until the next play. */
  follows: boolean;
  /** The bar of the piece a key chose a note in, which the note editor then shows: none once bars are chosen. */
  noteBar: number | null;
  /** What an edit took that the composer did not ask it to take, said over the bars, with the sections as that edit left them: it shows only while they are so, so not after an undo. */
  notice: { text: string; form: string } | null;
  copied: Copied | null;
  below: Below;
  /** The instrument the keyboard plays, as a click on a strip's name chooses it beside the mixer. */
  track: string | null;
  /** The instrument whose head the pointer is over, which shows all its toggles. */
  hovered: string | null;
  renaming: string | null;
  /** What is being typed in a field over the band, which hides under it: `section-` and its id, or `signature-` or `tempo-` and the bar of the mark. */
  typing: string | null;
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
  when: "You change the piece's page: its line of bars and marks, its instruments, mixer strips or note editor",
  doc:
    "A piece in sections of bars, as a score has them. `sections` is the form, in order: each has an `id` (`sec-` and a name), a `name` and its `bars` as runs of alike bars, so every bar has its own time signature and tempo: `{ \"id\": \"sec-theme\", \"name\": \"Theme\", \"bars\": [{ \"bars\": 4, \"signature\": \"3/4\", \"bpm\": 96 }, { \"bars\": 1, \"signature\": \"4/4\", \"bpm\": 96 }] }` is five bars, the last of 4/4. A run with `to` slows down or speeds up in a straight line from `bpm` to `to` over all its bars, and the bars after it keep their own `bpm`: `{ \"bars\": 2, \"signature\": \"3/4\", \"bpm\": 96, \"to\": 72 }` is a ritardando over two bars. A run with `fermata` has a fermata on that count of each of its bars, from 1 as the time signature counts, which makes it last `hold` beats longer, 1 when left out, in the bar's own beat as its tempo is said, and no longer than the slowest tempo, 10, lets that count last. `bpm` and `to` count quarter notes; the page says and takes a tempo in the bar's own beat, a dotted quarter in 6/8, 9/8 and 12/8. Each instrument is a `track` child; its part in a section is its `timeline` child named by the section's id, in `state/piece/<instrument>/<section id>.json`, its notes in ticks (960 a beat) from the start of that section, none past its end. An instrument with no music in a section has no part there. The piece lays the sections one after the other and sets, from its own `sections`, each part's `start` and `length` and the clock's tempo map and section names: change bars, time signatures and tempos in the piece's record, never in `project.json` or a part's `start` and `length`. After a change by hand it lays them out again by itself. Each instrument and each return `ret-*` plays into the fader `master`, then its limiter `limit` unless `limit_off`; `order` is the order of the instruments, `solo` names those that play alone; each of `sidechains` keys an effect, by its path such as `t00/chain/comp`, with what an instrument `from` plays at `tap`: into its effects, out of them or out of its fader. The page shows the whole piece on one line, as a score's continuous view: two fingers pan, cmd-scroll or pinch zooms across. Over a row per instrument a band of marks: each section's rehearsal letter, boxed, and its name at its first bar, every other section a shade lighter down the whole page; the tempo as one line, higher when faster, that steps where it changes, slopes over bars that slow down or speed up and breaks under a fermata, which sits over its count, with its words where it changes, `rit. to 60` or `accel. to 132`, the tempo they reach in the beat of the last; under it a ruler with a tick per count, the time signature, stacked, where it changes, and the bar numbers, every fifth or tenth while the bars are narrow. The piece opens whole unless its bars would be too narrow. Once its bar is off the left edge, the time signature and the tempo that apply stay said there. A click on a time signature or a tempo types it, `6/8` or `72` in the bar's own beat, for the bars up to the next that differs, which it chooses so they show, a right-click chooses its bar; T types the time signature and shift-T the tempo of the chosen bars, or of the bar that plays, which it then chooses; one undo step, also across sections. Alt-T, or a click on `rit.` or `accel.`, types the tempo the chosen bars reach at their end, from the tempo the first starts at; `Steady tempo` there, or a tempo typed with shift-T, ends it. A bar added or removed inside one stretches or shortens it. F puts a fermata on the last count of the last chosen bar, or opens the menu of the one it has, and Fermata in the menu of a right-click on a bar puts it on the count under the pointer; a click on a fermata picks how many beats longer it holds, moves it a beat earlier or later, or removes it, a drag moves it to another count. A click on a section's name chooses its bars, a double click or shift-R renames it, a right-click renames or duplicates it, types its time signature or tempo, moves, joins or deletes it, a drag moves it before the section it is let go on. A click in the band chooses that bar whole, in a row that bar of that instrument; shift-click or a drag takes in more bars and instruments. A choice may cross section lines; while the piece is stopped it puts the playhead on its first bar, while it plays it moves nothing that is heard and the page stops turning with the playhead until the next play, or Follow (alt-F), which shows in the corner then. Under chosen whole bars `+ Bar` adds one after them and `Remove` removes them, and under any choice `More` opens its menu, which with all the bars of one section chosen also leads to the section's; after the last bar `+ Bar` adds one, unless the last bar is chosen and has that button under it, and `+ Section` a section at the end. Loop (L), in the corner, loops the chosen bars, or the section that plays. Notes a shorter time signature removes are said over the band until the next edit, choice or undo. On what is chosen: cmd-C, cmd-X, cmd-V and cmd-D copy, cut, paste and repeat it (whole bars paste and repeat as new bars; music goes over the bars from the first chosen one on, each section keeping its own part, the last section getting bars where the piece is too short), delete removes whole bars, a section that loses all of them with them, or clears music, left and right choose the bar before or after, up and down the instrument above or below (up from the first chooses the whole bars), shift with them moves the end of the choice, taking more in or letting some go, cmd-shift-right takes in the bars up to the end of the section, cmd-A all bars, alt-shift-left and alt-shift-right choose the first bar of the section before or the next, alt-shift-up and alt-shift-down move the section, shift-B adds a bar after the chosen ones, alt-shift-B one before, alt-B types how many to add, M opens the menu of the chosen bars and shift-M that of the section, shift-S starts a section at the first chosen bar, unless one starts there, or with none chosen adds one at the end; a double click or enter opens the notes of a passage below; a right-click shows these, and the time signature and tempo to type. Below the rows shows one of: an instrument's or a return's devices (a click on its head; a double click names it, a right-click renames, colours or deletes it, a drag moves it; with nothing chosen enter renames it, delete deletes it, up and down choose another, alt-up and alt-down move it), the master's (its row), the mixer (the toggle on its row) or the notes of a part. In the note editor N adds a note, alt-left and alt-right choose the note before and the next as they sound, from the playhead on, and the editor shows its bar, shift-left and shift-right make the chosen notes shorter and longer, its Grid is the note value notes snap to, triplets too, Q quantizes, the arrows move the chosen notes, alt-up and alt-down change their velocity, cmd-A, cmd-C, cmd-X, cmd-V and cmd-D work on notes while some are chosen, shift-enter makes it large; it shows the part of the section of the first chosen bar, on those bars with one each side, empty where its instrument has no part yet, a note ends where its section does, and its first note makes the part. A click under the rows or past the last bar lets go of the chosen bars. Escape lets go of chosen notes, then the chosen effect, then chosen bars, then closes the editor, then what shows below. Space plays and pauses. The keys named here are the first keys of its `actions`, in `piece-keys.ts`.",
  state: fields,
  children: parts,
  memory: (): Memory => ({
    chosen: null, part: null, follows: true, noteBar: null, notice: null, copied: null, below: null, track: null, hovered: null, renaming: null, typing: null, returnsOpen: false, names: {}, effect: null, opened: false, editor: EDITOR, editorLarge: false,
    notes: new Set(), snap: BEAT / 4, lane: "velocity", length: BEAT / 2, copiedNotes: null, strength: 1, swing: 0, humanized: 0,
  }),
  actions: pieceActions,
  tick: ({ memory, clock, state }) => {
    if (clock.playing || memory.follows) return;
    memory.follows = true;
    // What was chosen as it played is where it plays from next, as a choice puts the playhead while it is stopped. Not after a stop, which went back to the start.
    const from = memory.chosen?.from;
    if (from !== undefined && clock.tick !== 0) clock.jump(barsOfPiece(state).starts[from] ?? 0);
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
    const { rows, strips, starts, all, form } = page;
    // A piece opens on its first instrument, chosen, with its panel: a click on an instrument shows the same.
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
      const now = repeat ? `${repeat.from}-${repeat.to}:${state.sections.map((section) => section.id)}:${starts().filter((tick) => tick <= repeat.to)}` : "";
      if (repeated.split(":")[0] === now.split(":")[0] && repeated !== now) untrack(() => clock.setRepeat(null));
      repeated = now;
    });

    // The editor at its largest leaves the marks, one row, the returns and the master over it.
    const largest = () => Math.max(tool.size.height - DIVIDER - MARKS - ROW - page.returnsHeight() - LOW_ROW, SHORTEST);
    const editorHeight = () => (memory.editorLarge ? largest() : clamp(memory.editor, SHORTEST, largest()));
    const partId = () => partSectionOf(tool)?.id ?? null;
    const place = () => form().places[partId() ?? ""];
    // The bars of the piece the editor opens on: the chosen ones with a bar each side, which it keeps when they are let go, and in a section with none chosen yet its first eight.
    let kept: { id: string | null; from: number; to: number } = { id: null, from: 0, to: 8 };
    const shownBars = memo(() => {
      const [id, chosen, note] = [partId(), memory.chosen, memory.noteBar];
      // A note a key chose outside them takes the editor to its bar.
      if (note !== null && (kept.id !== id || note < kept.from || note >= kept.to)) kept = { id, from: note - 1, to: note + 2 };
      else if (note === null && chosen) kept = { id, from: chosen.from - 1, to: chosen.to + 2 };
      else if (kept.id !== id) {
        const first = id === null ? 0 : (barsOfSection(untrack(all), id)?.from ?? 0);
        kept = { id, from: first, to: first + 8 };
      }
      return `${kept.from}:${kept.to}`;
    });
    /** The ticks of the part those bars are, inside its section, as it is now. */
    const shownTicks = (bars: string): readonly [number, number] => {
      const [pieceStarts, section, at] = [untrack(starts), untrack(() => barsOfSection(all(), partId() ?? "")), untrack(place)];
      const [from = 0, to = 8] = bars.split(":").map(Number);
      if (!section || !at) return [0, 0];
      const tickOf = (bar: number) => (pieceStarts[clamp(bar, section.from, section.to + 1)] ?? 0) - at.start;
      return [tickOf(from), tickOf(to)];
    };
    const editorOf = (name: string, id: string, shows: readonly [number, number]) => {
      const row = () => rows().find((each) => each.name === name);
      return (
        <ClipEditor
          clip={() => editedPart(tool)}
          bars={() => starts().map((tick) => tick - (place()?.start ?? 0))}
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
    const Hairline = ({ strong = false }: { strong?: boolean }) => <div style={{ height: 1, background: strong ? "border-strong" : "border", keep: true }} />;
    const Rule = () => <div style={{ width: 1, background: "border", keep: true }} />;

    return (
      <Page flush header={<Transport computerKeys tempo={false} />}>
        <Hairline strong />
        <div style={{ direction: "row", grow: true }}>
          <Heads page={page} />
          <Rule />
          <div style={{ grow: true }}>
            <Marks page={page} />
            <Rows page={page} />
          </div>
        </div>
        {() => {
          const shown = memory.below;
          const open = shown?.kind === "editor" ? partId() : null;
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
