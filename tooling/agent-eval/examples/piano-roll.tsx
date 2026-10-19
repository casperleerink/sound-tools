import { Group, h, Label, Line, notes, Rect, Space, tool, type Drag, type Key, type Note, type Press } from "./sdk";

const BEAT = 960;
const BAR = 4 * BEAT;
const BARS = 32;
const END = BARS * BAR;
const STEP = BEAT / 4;
const ROW = 12;
const KEYS = 44;
const RULER = 22;
const LANE = 72;
// Points per tick at zoom 1: a beat is 96 points.
const ACROSS = 0.1;
const HANDLE = 90;

const COLOR = {
  back: "#121317", dark: "#17191e", bar: "#3a3e48", beat: "#23262d", note: "#a9b1ff", hover: "#c8cdff",
  chosen: "#e9ebef", clear: "#00000000", playhead: "#7ee0a0", marquee: "#a9b1ff33", text: "#7c808c",
  white: "#d8dbe2", black: "#2a2d35",
} as const;

const isBlack = (pitch: number) => [1, 3, 6, 8, 10].includes(pitch % 12);
const rowOf = (pitch: number) => 127 - pitch;
const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);
const barHeight = (velocity: number) => 4 + (velocity / 127) * (LANE - 10);

type Memory = { selected: Set<number>; clipboard: Note[]; length: number };

/** `notes` with `chosen` moved by `ticks` and `pitches`, or copied there: the notes, and which are chosen after. */
function moved(notes: readonly Note[], chosen: Set<number>, ticks: number, pitches: number, copy: boolean) {
  const shifted = (note: Note) => ({ ...note, start: Math.max(note.start + ticks, 0), pitch: clamp(note.pitch + pitches, 0, 127) });
  if (!copy) {
    return { notes: notes.map((note, index) => (chosen.has(index) ? shifted(note) : note)), chosen };
  }
  const copies = [...chosen].map((index) => shifted(notes[index]));
  return { notes: [...notes, ...copies], chosen: new Set(copies.map((_, index) => notes.length + index)) };
}

/** The notes `chosen` names, from the earliest one. */
function copied(notes: readonly Note[], chosen: Set<number>): Note[] {
  const picked = [...chosen].sort((a, b) => a - b).map((index) => notes[index]);
  const first = Math.min(...picked.map((note) => note.start));
  return picked.map((note) => ({ ...note, start: note.start - first }));
}

/** A key for each note from what it is, which a delete does not shift: start, pitch, and which of the same. */
function keysOf(notes: readonly Note[]): string[] {
  const seen = new Map<string, number>();
  return notes.map((note) => {
    const at = `${note.start}-${note.pitch}`;
    const count = seen.get(at) ?? 0;
    seen.set(at, count + 1);
    return `${at}-${count}`;
  });
}

const span = (picked: readonly Note[]) => Math.ceil(Math.max(...picked.map((note) => note.start + note.length)) / STEP) * STEP;

export const pianoRoll = tool({
  name: "piano-roll",
  title: "Piano roll",
  when: "You change the piano roll page: how notes are drawn, moved and edited",
  doc:
    "A piano roll of the record's `notes`. Double-click to add a note or remove one; drag notes to move them on the grid of sixteenths, alt to copy; drag a note's right end to resize; drag on empty space to select, shift to add; drag a bar in the lane under it to change velocity. Keys: delete, cmd-A, cmd-C, cmd-X, cmd-V, cmd-D, the arrows, escape.",
  kind: "source",
  state: { notes: notes() },
  transport: true,
  memory: (): Memory => ({ selected: new Set(), clipboard: [], length: BEAT / 2 }),
  sound: () => 0,
  onKey: ({ state, memory, update }, key: Key) => {
    if (!key.down) {
      return;
    }
    const chosen = memory.selected;
    const all = state.notes;
    const edit = (label: string, change: (notes: Note[]) => { notes: Note[]; chosen: Set<number> }) =>
      update(label, ({ state }) => {
        const result = change(state.notes);
        state.notes = result.notes;
        memory.selected = result.chosen;
      });
    const without = (notes: Note[]) => ({ notes: notes.filter((_, index) => !chosen.has(index)), chosen: new Set<number>() });
    if (key.cmd && key.key === "a") {
      memory.selected = new Set(all.map((_, index) => index));
    } else if (key.cmd && (key.key === "c" || key.key === "x") && chosen.size > 0) {
      memory.clipboard = copied(all, chosen);
      if (key.key === "x") {
        edit("Cut notes", without);
      }
    } else if (key.cmd && key.key === "v" && memory.clipboard.length > 0) {
      // After the last note, so a paste never lands on what is there.
      const at = all.length > 0 ? span(all) : 0;
      const pasted = memory.clipboard.map((note) => ({ ...note, start: note.start + at }));
      edit("Paste notes", (notes) => ({ notes: [...notes, ...pasted], chosen: new Set(pasted.map((_, index) => notes.length + index)) }));
    } else if (key.cmd && key.key === "d" && chosen.size > 0) {
      const length = span(copied(all, chosen));
      edit("Duplicate notes", (notes) => moved(notes, chosen, length, 0, true));
    } else if ((key.key === "backspace" || key.key === "delete") && chosen.size > 0) {
      edit("Delete notes", without);
    } else if (key.key === "escape") {
      memory.selected = new Set();
    } else if (chosen.size > 0 && ["left", "right", "up", "down"].includes(key.key)) {
      const ticks = key.key === "left" ? -STEP : key.key === "right" ? STEP : 0;
      const pitches = key.key === "up" ? 1 : key.key === "down" ? -1 : 0;
      edit("Move notes", (notes) => moved(notes, chosen, ticks, pitches, false));
    }
  },
  page: ({ state, memory, update, now, size }) => {
    const all = state.notes;
    const chosen = memory.selected;
    const width = size.width - KEYS;
    const height = size.height - RULER - LANE - 2;
    const [move, resize, velocity, marquee] = [now.drag("move"), now.drag("resize"), now.drag("velocity"), now.drag("marquee")];

    const edit = (label: string, change: (notes: Note[]) => { notes: Note[]; chosen: Set<number> }) =>
      update(label, ({ state }) => {
        const result = change(state.notes);
        state.notes = result.notes;
        memory.selected = result.chosen;
      });
    const press = (index: number) => ({ clicks, shift }: Press) => {
      if (clicks === 2) {
        edit("Remove a note", (notes) => ({ notes: notes.filter((_, each) => each !== index), chosen: new Set() }));
      } else if (shift) {
        const next = new Set(chosen);
        if (!next.delete(index)) next.add(index);
        memory.selected = next;
      } else if (!chosen.has(index)) {
        memory.selected = new Set([index]);
      }
    };
    // One of each for every note: a drag shared by many shapes crosses once.
    const moveDrag: Drag = {
      id: "move",
      snap: { x: STEP, y: 1, grid: true },
      onEnd: ({ dx, dy, alt }) => edit(alt ? "Copy notes" : "Move notes", (notes) => moved(notes, memory.selected, dx, -dy, alt)),
    };
    const resizeDrag: Drag = {
      id: "resize",
      axis: "x",
      snap: { x: STEP, grid: true },
      onEnd: ({ dx }) =>
        edit("Resize notes", (notes) => {
          const changed = notes.map((note, index) => (memory.selected.has(index) ? { ...note, length: Math.max(note.length + dx, STEP / 4) } : note));
          const last = [...memory.selected].at(-1);
          if (last !== undefined) memory.length = changed[last].length;
          return { notes: changed, chosen: memory.selected };
        }),
    };
    const velocityDrag: Drag = {
      id: "velocity",
      axis: "y",
      onEnd: ({ dy }) => {
        const change = (-dy / (LANE - 10)) * 127;
        edit("Change velocity", (notes) => ({
          notes: notes.map((note, index) => (memory.selected.has(index) ? { ...note, velocity: Math.round(clamp(note.velocity + change, 1, 127)) } : note)),
          chosen: memory.selected,
        }));
      },
    };
    const marqueeDrag: Drag = {
      id: "marquee",
      onEnd: ({ startX, startY, x, y, shift }) => {
        const [left, right] = [Math.min(startX, x), Math.max(startX, x)];
        const [top, bottom] = [Math.min(startY, y), Math.max(startY, y)];
        const inside = all.flatMap((note, index) => {
          const row = rowOf(note.pitch);
          return note.start < right && note.start + note.length > left && row < bottom && row + 1 > top ? [index] : [];
        });
        memory.selected = new Set([...(shift ? chosen : []), ...inside]);
      },
    };
    const pressEmpty = ({ clicks, shift, x, y }: Press) => {
      if (clicks === 2) {
        const note = { start: Math.floor(x / STEP) * STEP, length: memory.length, pitch: clamp(127 - Math.floor(y), 0, 127), velocity: 100 };
        edit("Add a note", (notes) => ({ notes: [...notes, note], chosen: new Set([notes.length]) }));
      } else if (!shift) {
        memory.selected = new Set();
      }
    };

    const keys = keysOf(all);
    const noteShapes = (index: number, note: Note, picked: boolean) => {
      const row = rowOf(note.pitch);
      const end = note.start + note.length;
      const handle = Math.min(HANDLE, note.length / 3);
      const onPress = press(index);
      return [
        <Rect key={`n${keys[index]}`} x={note.start} y={row} width={picked ? resize.dx.plus(note.length).max(STEP / 4) : note.length} height={1} radius={2}
          color={picked ? COLOR.chosen : COLOR.note} hover={picked ? undefined : COLOR.hover} cursor="grab" drag={moveDrag} onPress={onPress} />,
        <Rect key={`r${keys[index]}`} x={picked ? resize.dx.plus(end - handle) : end - handle} y={row} width={handle} height={1}
          color={COLOR.clear} cursor="ew-resize" drag={resizeDrag} onPress={onPress} anchor={{ x: end }} />,
      ];
    };
    const bar = (index: number, note: Note, picked: boolean) => (
      <Rect key={`v${keys[index]}`} x={note.start} y={LANE - barHeight(note.velocity)} width={now.pt(6)} height={LANE}
        color={picked ? COLOR.chosen : COLOR.note} cursor="ns-resize" drag={velocityDrag} onPress={press(index)} />
    );
    const across = { unit: { x: ACROSS, y: 1 }, extent: { x: [0, END] as const }, zoom: { x: [0.05, 8] as const } };
    const rows = Array.from({ length: 128 }, (_, row) => 127 - row);
    const bars = Array.from({ length: BARS + 1 }, (_, bar) => bar * BAR);
    const beats = Array.from({ length: BARS * 4 }, (_, beat) => beat * BEAT).filter((tick) => tick % BAR !== 0);
    const picked = all.flatMap((note, index) => (chosen.has(index) ? [[index, note] as const] : []));
    const rest = all.flatMap((note, index) => (chosen.has(index) ? [] : [[index, note] as const]));

    return (
      <div style={{ gap: 1, background: COLOR.back }}>
        <div style={{ direction: "row" }}>
          <div style={{ width: KEYS }} />
          <Space key="ruler" width={width} height={RULER} {...across} follow={{ x: "roll" }}>
            {bars.map((tick) => <Line from={[tick, 10]} to={[tick, RULER]} color={COLOR.bar} width={1} />)}
            {bars.slice(0, BARS).map((tick, bar) => <Label x={tick + 40} y={3} text={String(bar + 1)} size={10} color={COLOR.text} />)}
            <Line from={[now.tick, 0]} to={[now.tick, RULER]} color={COLOR.playhead} width={2} />
          </Space>
        </div>
        <div style={{ direction: "row" }}>
          <Space key="keys" width={KEYS} height={height} unit={{ x: 1, y: ROW }} extent={{ y: [0, 128] }} scroll={{ y: 40 }} follow={{ y: "roll" }}>
            {rows.map((pitch) => (
              <Rect x={0} y={rowOf(pitch) + 0.04} width={isBlack(pitch) ? 28 : KEYS} height={0.92} color={isBlack(pitch) ? COLOR.black : COLOR.white} />
            ))}
            {rows.filter((pitch) => pitch % 12 === 0).map((pitch) => (
              <Label x={KEYS - 4} y={rowOf(pitch) + 0.1} text={`C${pitch / 12 - 1}`} size={9} color={COLOR.back} align="right" />
            ))}
          </Space>
          <Space key="roll" width={width} height={height} background={COLOR.back} unit={{ x: ACROSS, y: ROW }} extent={{ x: [0, END], y: [0, 128] }}
            zoom={{ x: [0.05, 8] }} scroll={{ y: 40 }} drag={marqueeDrag} onPress={pressEmpty}>
            {rows.filter(isBlack).map((pitch) => <Rect x={0} y={rowOf(pitch)} width={END} height={1} color={COLOR.dark} />)}
            {beats.map((tick) => <Line from={[tick, 0]} to={[tick, 128]} color={COLOR.beat} width={1} />)}
            {bars.map((tick) => <Line from={[tick, 0]} to={[tick, 128]} color={COLOR.bar} width={1} />)}
            {rest.map(([index, note]) => noteShapes(index, note, false))}
            <Group x={move.dx} y={move.dy}>
              {picked.map(([index, note]) => noteShapes(index, note, true))}
            </Group>
            <Rect x={marquee.startX.min(marquee.x)} y={marquee.startY.min(marquee.y)} width={marquee.dx.abs()} height={marquee.dy.abs()}
              color={COLOR.marquee} opacity={marquee.active} />
            <Line from={[now.tick, 0]} to={[now.tick, 128]} color={COLOR.playhead} width={2} />
          </Space>
        </div>
        <div style={{ direction: "row" }}>
          <div style={{ width: KEYS, padding: 4, color: COLOR.text, fontSize: 10 }}>Vel</div>
          <Space key="velocity" width={width} height={LANE} {...across} follow={{ x: "roll" }}>
            {rest.map(([index, note]) => bar(index, note, false))}
            <Group y={velocity.dy}>{picked.map(([index, note]) => bar(index, note, true))}</Group>
          </Space>
        </div>
      </div>
    );
  },
});
