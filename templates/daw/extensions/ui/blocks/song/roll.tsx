import { h, live, memo, type Color, type Frozen, type Note, type Pointer } from "../../../sdk";
import { noteCommand, noteKey, pitchName, STRENGTHS, SWINGS, type NoteCommand } from "../../../song/notes";
import { BAR, BEAT, dragGrid, linesIn, placeOf } from "../../../song/time";
import { MenuList, type Item } from "../../menu";
import { EmptyState } from "../empty-state";
import { barsOf, BEFORE, END, MOVE, PITCH, playing, SHORTEST, signatureOf, START, ZOOM, type Roll } from "./roll-view";
import { Playhead } from "../time";

/** The fewest points between two lines of the grid: closer, they do not show. How many ticks wide the ends of a note are that resize it. */
const [ROOM, HANDLE] = [6, 90];
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
  // Held at the clip's ends, so a playhead outside the clip moves no roll.
  const follow = playing(roll).at.max(0).min(roll.length);
  return (
    <box grow={1} h="full">
      <box key={key} label="Notes" w="full" h="full" clip plane={{ x: roll.across, y: PITCH, zoom: ZOOM, follow }} drag={{ x: true, y: true }}
        onPress={(p) => !p.right && edits.pressEmpty(inRows(p))} onDragEnd={(p) => edits.marquee(inRows(p))}>
        <Grid roll={roll} />
        {barsOf(state.notes).map(([bar, notes]) =>
          memo(`notes-${key}-${bar}`, [signatureOf(roll, notes), top, roll.across, roll.colour, roll.page.song.memory.snap, roll.runs, roll.state.start], () => (
            <box>{notes.map(({ note, index }) => noteParts(roll, note, index))}</box>
          )),
        )}
        <box place={[left, upper, marquee.dx.abs(), marquee.dy.abs()]} radius="sm" fill="focus/15" border="focus" opacity={marquee.active} inert />
        <Playhead plane={key} {...playing(roll)} />
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
  return memo(`grid-${key}`, [top, end, length, snap, roll.runs, state.start, loop, colour], () => {
    const width = end + BEFORE;
    const shown = (step: number) => live.plane(key).zoom.ge(ROOM / step);
    const lines = (ticks: number[]) => ticks.flatMap((tick) => [tick + BEFORE, 0, tick + BEFORE, 128]);
    /** The lines of the song's grid `step` over the roll, in the clip's ticks. */
    const every = (step: number) => linesIn(roll.runs, state.start - BEFORE, state.start + end, step).map((tick) => tick - state.start);
    const beats = every(BEAT);
    const area = [-BEFORE, -top, width, 128] as const;
    return (
      <box key={`${key}-grid`}>
        <shapes place={[...area]} rects={[{ color: "fill/40", rects: Array.from({ length: 128 }, (_, row) => row).filter((row) => BLACK.includes((127 - row) % 12)).flatMap((row) => [0, row, width, 1]) }]} />
        {snap > 0 && snap !== BAR && snap !== BEAT && <shapes place={[...area]} opacity={shown(snap).times(0.5)} lines={[{ color: "border", width: 1, points: lines(every(snap).filter((tick) => !beats.includes(tick))) }]} />}
        <shapes place={[...area]} opacity={shown(BEAT)} lines={[{ color: "border", width: 1, points: lines(beats) }]} />
        <shapes place={[...area]} lines={[{ color: "border-strong", width: 1, points: lines(bars.map((bar) => bar.tick)) }]} />
        {state.loop && <shapes place={[...area]} rects={[{ color: `${colour}/25` as Color, rects: state.notes.flatMap((note) => [note.start + length + BEFORE, 127 - note.pitch, note.length, 1]) }]} />}
        <box place={[-BEFORE, -top, BEFORE, 128]} fill="background/70" inert />
        <box place={[length, -top, end - length, 128]} fill="background/70" inert />
      </box>
    );
  });
};

/**
 * A note: a point short of its row, its shade its velocity, bright while chosen. The chosen notes
 * share their drags, so they move and resize as one; a drag of one not chosen chooses it alone.
 * A chosen note's ends resize it: only a chosen one has them, as a zoom walks every element.
 */
function noteParts(roll: Roll, note: Frozen<Note>, index: number) {
  const chosen = roll.chosen.has(noteKey(note));
  const { top, edits, colour, page } = roll;
  const snap = page.song.memory.snap;
  const key = `note-${index}`;
  const row = 127 - note.pitch - top;
  const gridOf = (tick: number) => dragGrid(roll.runs, roll.state.start + tick, snap, [1, row]);
  const [move, front, back] = [live.drag(chosen ? MOVE : key), live.drag(START), live.drag(END)];
  const press = (p: Pointer) => (p.clicks === 2 ? edits.remove(note) : edits.press(note, p));
  const name = `${pitchName(note.pitch)} at ${placeOf(page.song.clock.tempo_map.time_signatures, roll.state.start + note.start).join(".")}`;
  const shade = Math.round(35 + (65 * note.velocity) / 127);
  const [x, y] = [move.dx.plus(chosen ? front.dx.plus(note.start) : note.start), move.dy.plus(row)];
  const length = chosen ? back.dx.minus(front.dx).plus(note.length).max(SHORTEST) : note.length;
  // An end is a third of the note at most, so a short note still moves; in ticks, so a scroll moves none of it.
  const handle = Math.min(HANDLE, note.length / 3);
  const end = (side: "start" | "end") => (
    <box key={`${key}-${side}`} label={`${side === "start" ? "Start" : "End"} of note ${name}`} place={[side === "start" ? x : x.plus(length).minus(handle), y, handle, 1]}
      drag={{ x: true, ...gridOf(side === "start" ? note.start : note.start + note.length), id: side === "start" ? START : END }}
      onPress={press} onDragEnd={(p) => (side === "start" ? edits.resizeStart(p) : edits.resize(p))} />
  );
  return (
    <box key={key} label={`Note ${name}`} role="button" value={chosen}
      place={[x, y, length, (PITCH - 1) / PITCH]} radius="sm"
      fill={chosen ? "text" : (`${colour}/${shade}` as Color)} border={colour} drag={{ x: true, y: true, ...gridOf(note.start), id: chosen ? MOVE : key }}
      onPress={press} onDragEnd={(p) => (!chosen && edits.choose(note), edits.move(p))}>
      {chosen && end("start")}
      {chosen && end("end")}
    </box>
  );
}


/** The note menu: what to do with the chosen notes, or with all while none is, with the keys that do the same. */
export function noteItems(roll: Roll): Item[] {
  const { song } = roll.page;
  const { memory } = song;
  const keys = (action: string) => song.shortcut(action);
  const run = (command: NoteCommand, action: string, text: string, danger?: true): Item => ({ text, keys: keys(action), onPress: () => void noteCommand(song, command), ...(danger ? { danger } : {}) });
  const chosen = roll.state.notes.some((note) => roll.chosen.has(noteKey(note)));
  const first: Item[] = chosen
    ? [run("cut", "cut_notes", "Cut"), run("copy", "copy_notes", "Copy"), run("paste", "paste_notes", "Paste"), run("duplicate", "duplicate_notes", "Duplicate"), run("delete", "delete_notes", "Delete", true)]
    : [run("add", "add_note", "Add a note"), run("paste", "paste_notes", "Paste"), run("all", "select_all_notes", "Choose all")];
  return [
    ...first,
    { ...run("quantize", "quantize_notes", chosen ? "Quantize" : "Quantize all"), line: true },
    ...STRENGTHS.map((strength) => ({ text: `Strength ${strength * 100}%`, checked: memory.strength === strength, onPress: () => void (memory.strength = strength) })),
    ...SWINGS.map(({ label, swing }) => ({ text: swing === 0 ? label : `${label} swing`, checked: memory.swing === swing, onPress: () => void (memory.swing = swing) })),
    { text: "Humanize", line: true, onPress: () => roll.edits.operate("humanize") },
    { text: "Legato", onPress: () => roll.edits.operate("legato") },
  ];
}
