import { h, live, memo, type Color, type Frozen, type Note, type Pointer } from "../../../sdk";
import { noteCommand, noteKey, pitchName, STRENGTHS, SWINGS, type NoteCommand } from "../../../song/notes";
import { BEAT, placeOf } from "../../../song/time";
import { keysOf } from "../../kbd";
import { MenuList, type Item } from "../../menu";
import { EmptyState } from "../empty-state";
import { barsOf, BEFORE, END, MOVE, PITCH, playheadOf, SHORTEST, signatureOf, START, ticksOf, zoomOf, type Roll } from "./roll-view";

/** The fewest points between two lines of the grid: closer, they do not show. */
const ROOM = 6;
const BLACK = [1, 3, 6, 8, 10];

/**
 * The notes of the clip on a grid of pitches: a double click adds one, a drag over empty space
 * chooses what it covers, a drag of a note moves the chosen ones, of an end resizes them. The
 * right button, or the menu key, opens the note menu.
 */
export const RollPlane = ({ roll }: { roll: Roll }) => {
  const { key, top, edits, state } = roll;
  const marquee = live.drag(key);
  const [left, upper] = [marquee.x.plus(marquee.dx.min(0)), marquee.y.plus(marquee.dy.min(0))];
  const inRows = (p: Pointer) => ({ ...p, y: p.y + top });
  return (
    <box grow={1} h="full">
      <box key={key} label="Notes" w="full" h="full" clip plane={{ x: roll.across, y: PITCH, zoom: zoomOf(roll) }} drag={{ x: true, y: true }}
        onPress={(p) => !p.right && edits.pressEmpty(inRows(p))} onDragEnd={(p) => edits.marquee(inRows(p))}>
        <Grid roll={roll} />
        {barsOf(state.notes).map(([bar, notes]) =>
          memo(`notes-${key}-${bar}`, [signatureOf(roll, notes), top, roll.across, roll.colour, roll.page.song.memory.snap, roll.runs], () => (
            <box>{notes.flatMap(({ note, index }) => noteParts(roll, note, index))}</box>
          )),
        )}
        <box place={[left, upper, marquee.dx.abs(), marquee.dy.abs()]} radius="sm" fill="fill/60" border="border-strong" opacity={marquee.active} inert />
        <Playhead roll={roll} />
        <MenuList id="note-menu" label="Note menu" items={noteItems(roll)} open="menu" w={220} />
      </box>
      {state.notes.length === 0 && (
        <box key="notes-empty" inset={[0, 0, 0, 0]} inert>
          <EmptyState title="No notes yet" text="Double-click to add a note, or press N." />
        </box>
      )}
    </box>
  );
};

/** The rows of the black keys, the lines of the bars, the beats and the snap, and the clip's bounds; a loop shows its next round, dimmed. */
const Grid = ({ roll }: { roll: Roll }) => {
  const { key, top, end, length, state, bars, colour } = roll;
  const snap = roll.page.song.memory.snap;
  const loop = state.loop ? state.notes.map(noteKey).join(",") : "";
  return memo(`grid-${key}`, [top, end, length, snap, roll.runs, roll.across, loop, colour], () => {
    const width = end + BEFORE;
    const zoom = live.plane(key).zoom;
    const shown = (step: number) => zoom.ge(ROOM / (step * roll.across));
    const lines = (ticks: number[]) => ticks.flatMap((tick) => [tick + BEFORE, 0, tick + BEFORE, 128]);
    const every = (step: number, over: number) => Array.from({ length: Math.ceil(end / step) }, (_, at) => at * step).filter((tick) => tick % over !== 0);
    const area = [-BEFORE, -top, width, 128] as const;
    return (
      <box>
        <shapes place={[...area]} rects={[{ color: "fill/40", rects: Array.from({ length: 128 }, (_, row) => row).filter((row) => BLACK.includes((127 - row) % 12)).flatMap((row) => [0, row, width, 1]) }]} />
        {snap > 0 && snap < BEAT && <shapes place={[...area]} opacity={shown(snap).times(0.5)} lines={[{ color: "border", width: 1, points: lines(every(snap, BEAT)) }]} />}
        <shapes place={[...area]} opacity={shown(BEAT)} lines={[{ color: "border", width: 1, points: lines(every(BEAT, 4 * BEAT).filter((tick) => !bars.some((bar) => bar.tick === tick))) }]} />
        <shapes place={[...area]} lines={[{ color: "border-strong", width: 1, points: lines(bars.map((bar) => bar.tick)) }]} />
        {state.loop && <shapes place={[...area]} rects={[{ color: `${colour}/25` as Color, rects: state.notes.flatMap((note) => [note.start + length + BEFORE, 127 - note.pitch, note.length, 1]) }]} />}
        <box place={[-BEFORE, -top, BEFORE, 128]} fill="background/70" inert />
        <box place={[length, -top, end - length, 128]} fill="background/70" inert />
      </box>
    );
  });
};

/**
 * A note: a point short of its length and its row, its shade its velocity, bright while chosen.
 * The chosen notes share their drags, so they move and resize as one; a drag of one not chosen
 * chooses it alone.
 */
function noteParts(roll: Roll, note: Frozen<Note>, index: number) {
  const chosen = roll.chosen.has(noteKey(note));
  const { top, edits, colour, page } = roll;
  const snap = page.song.memory.snap;
  const key = `note-${index}`;
  const row = 127 - note.pitch - top;
  const shared = (id: string, own: string) => (chosen ? id : own);
  const [move, front, back] = [live.drag(shared(MOVE, key)), live.drag(shared(START, `${key}-start`)), live.drag(shared(END, `${key}-end`))];
  const x = move.dx.plus(front.dx).plus(note.start);
  const y = move.dy.plus(row);
  const w = back.dx.minus(front.dx).plus(note.length).max(SHORTEST);
  const handle = Math.min(90, note.length / 4);
  const own = () => !chosen && edits.choose(note);
  const press = (p: Pointer) => (p.clicks === 2 ? edits.remove(note) : edits.press(note, p));
  const name = `${pitchName(note.pitch)} at ${placeOf(page.song.clock.tempo_map.time_signatures, roll.state.start + note.start).join(".")}`;
  const marquee = live.drag(roll.key);
  const across = [marquee.x, marquee.x.plus(marquee.dx)];
  const down = [marquee.y, marquee.y.plus(marquee.dy)];
  const boxed = marquee.active
    .times(across[0]!.min(across[1]!).lt(note.start + note.length))
    .times(across[0]!.max(across[1]!).gt(note.start))
    .times(down[0]!.min(down[1]!).lt(row + 1))
    .times(down[0]!.max(down[1]!).gt(row));
  const shade = Math.round(35 + (65 * note.velocity) / 127);
  return [
    <box key={key} label={`Note ${name}`} role="button" checked={chosen} value={note.velocity} place={[x, y, w.minus(ticksOf(roll, 1)), (PITCH - 1) / PITCH]} radius="sm"
      fill={chosen ? "text" : (`${colour}/${shade}` as Color)} border={colour} drag={{ x: true, y: true, snap: [snap, 1], from: [note.start, row], id: shared(MOVE, key) }}
      onPress={press} onDragEnd={(p) => (own(), edits.move(p))}>
      <box inset={[0, 0, 0, 0]} radius="sm" fill="text/50" opacity={boxed} inert />
    </box>,
    <box key={`${key}-start`} label={`Start of note ${name}`} place={[x, y, handle, 1]} drag={{ x: true, snap: [snap, 1], from: [note.start, row], id: shared(START, `${key}-start`) }}
      onPress={press} onDragEnd={(p) => (own(), edits.resizeStart(p))} />,
    <box key={`${key}-end`} label={`End of note ${name}`} place={[x.plus(w).minus(handle), y, handle, 1]} drag={{ x: true, snap: [snap, 1], from: [note.start + note.length, row], id: shared(END, `${key}-end`) }}
      onPress={press} onDragEnd={(p) => (own(), edits.resize(p))} />,
  ];
}

/** Where the piece plays, while it plays inside the clip. */
const Playhead = ({ roll }: { roll: Roll }) => {
  const { at, inside } = playheadOf(roll);
  return <box place={[at, 64 - roll.top]} w={1} h={128 * PITCH} fill="text" opacity={inside} inert />;
};

/** The note menu: what to do with the chosen notes, or with all while none is, with the keys that do the same. */
export function noteItems(roll: Roll): Item[] {
  const { song } = roll.page;
  const { memory } = song;
  const keys = (action: string) => {
    const key = song.shortcut(action);
    return key === null ? undefined : keysOf(key);
  };
  const run = (command: NoteCommand, action: string, text: string, danger?: true): Item => ({ text, keys: keys(action), onPress: () => void noteCommand(song, command), ...(danger ? { danger } : {}) });
  const chosen = roll.state.notes.some((note) => roll.chosen.has(noteKey(note)));
  const first: Item[] = chosen
    ? [run("cut", "cut_notes", "Cut"), run("copy", "copy_notes", "Copy"), run("paste", "paste_notes", "Paste"), run("duplicate", "duplicate_notes", "Duplicate"), run("delete", "delete_notes", "Delete", true)]
    : [run("add", "add_note", "Add a note"), run("paste", "paste_notes", "Paste"), run("all", "select_all_notes", "Choose all")];
  return [
    ...first,
    { ...run("quantize", "quantize_notes", chosen ? "Quantize" : "Quantize all"), divider: true },
    ...STRENGTHS.map((strength) => ({ text: `Strength ${strength * 100}%`, checked: memory.strength === strength, onPress: () => void (memory.strength = strength) })),
    ...SWINGS.map(({ label, swing }) => ({ text: swing === 0 ? label : `${label} swing`, checked: memory.swing === swing, onPress: () => void (memory.swing = swing) })),
    { text: "Humanize", divider: true, onPress: () => roll.edits.operate("humanize") },
    { text: "Legato", onPress: () => roll.edits.operate("legato") },
  ];
}
