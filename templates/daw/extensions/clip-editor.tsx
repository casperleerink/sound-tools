import { For, Fragment, h, Label, Line, memo, now, Rect, selector, signal, Space, type Accessor, type Drag, type FilledStateOfTool, type Frozen, type Key, type Memory, type MenuItem, type Note, type Press } from "./sdk";

// The note editor of one timeline: its notes on a grid of pitches, and under them a lane of
// their velocities or of one of its expression lanes. Times are ticks from the clip's start.
const BEAT = 960;
const BAR = 4 * BEAT;
const ROW = 8;
const KEYS = 36;
const LANE = 64;
const ACROSS = 0.1;
const SNAPS = [
  { label: "Bar", ticks: BAR },
  { label: "1/4", ticks: BEAT },
  { label: "1/8", ticks: BEAT / 2 },
  { label: "1/16", ticks: BEAT / 4 },
  { label: "1/32", ticks: BEAT / 8 },
  { label: "Off", ticks: 0 },
] as const;
const LANES = ["velocity", "pedal", "bend", "mod_wheel", "pressure"] as const;
const LANE_NAMES: Record<Lane, string> = { velocity: "Velocity", pedal: "Pedal", bend: "Bend", mod_wheel: "Mod", pressure: "Pressure" };

const COLOR = {
  back: "#121317", dark: "#17191e", bar: "#3a3e48", beat: "#23262d", note: "#a9b1ff", hover: "#c8cdff",
  chosen: "#e9ebef", clear: "#00000000", playhead: "#7ee0a0", marquee: "#a9b1ff33", text: "#7c808c",
  white: "#d8dbe2", black: "#2a2d35", off: "#2a2d35", on: "#a9b1ff",
} as const;

export type Lane = (typeof LANES)[number];
export type Clip = FilledStateOfTool<"timeline">;
/** What the editor keeps in the song's memory: the chosen notes, the grid, the lane shown, the last note's length and the copied notes, from 0. */
export interface EditorMemory {
  notes: Set<number>;
  snap: number;
  lane: Lane;
  length: number;
  copied: Note[] | null;
}

const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);
const isBlack = (pitch: number) => [1, 3, 6, 8, 10].includes(pitch % 12);
const rowOf = (pitch: number) => 127 - pitch;
const snapped = (tick: number, snap: number) => (snap > 0 ? Math.floor(tick / snap) * snap : Math.round(tick));

/** A point of a lane as the record holds it: the pedal counts from `start`, the others from `tick`. */
type Point = { at: number; value: number };
const LIMITS: Record<Exclude<Lane, "velocity">, [number, number]> = { pedal: [0, 127], bend: [-8192, 8191], mod_wheel: [0, 127], pressure: [0, 127] };
function pointsOf(clip: Frozen<Clip>, lane: Exclude<Lane, "velocity">): Point[] {
  // A view reads the record as the file holds it: a lane it leaves out is empty.
  return lane === "pedal" ? (clip.pedal ?? []).map(({ start, value }) => ({ at: start, value })) : (clip[lane] ?? []).map(({ tick, value }) => ({ at: tick, value }));
}
function setPoints(clip: Clip, lane: Exclude<Lane, "velocity">, points: Point[]) {
  // One point per tick, in order, as the record needs them.
  const sorted = [...new Map(points.map((point) => [point.at, point])).values()].sort((a, b) => a.at - b.at);
  if (lane === "pedal") clip.pedal = sorted.map(({ at, value }) => ({ start: at, value: Math.round(value) }));
  else clip[lane] = sorted.map(({ at, value }) => ({ tick: at, value: Math.round(value) }));
}
/** Where a value sits up a lane, 0 to 1, and back. */
const upOf = (lane: Exclude<Lane, "velocity">, value: number) => (value - LIMITS[lane][0]) / (LIMITS[lane][1] - LIMITS[lane][0]);
const valueAt = (lane: Exclude<Lane, "velocity">, up: number) => LIMITS[lane][0] + clamp(up, 0, 1) * (LIMITS[lane][1] - LIMITS[lane][0]);

/** `notes` with `chosen` moved by `ticks` and `pitches`, or copied there: the notes, and which are chosen after. */
function moved(notes: readonly Note[], chosen: ReadonlySet<number>, ticks: number, pitches: number, copy: boolean) {
  const shifted = (note: Note) => ({ ...note, start: Math.max(note.start + ticks, 0), pitch: clamp(note.pitch + pitches, 0, 127) });
  if (!copy) return { notes: notes.map((note, index) => (chosen.has(index) ? shifted(note) : note)), chosen };
  const copies = [...chosen].map((index) => shifted(notes[index]!));
  return { notes: [...notes, ...copies], chosen: new Set(copies.map((_, index) => notes.length + index)) };
}

/**
 * Changes the notes of the clip as `next` says, as one undo step, and chooses what it chose. A
 * note that would start at or past the end of the clip is left out: the timeline plays none there.
 */
function changer(memory: Memory<EditorMemory>, edit: EditorProps["edit"]) {
  return (label: string, next: (notes: Note[]) => { notes: Note[]; chosen: ReadonlySet<number> }) =>
    edit(label, (draft) => {
      const result = next(draft.notes);
      const end = draft.length ?? Infinity;
      const kept = new Map<number, number>();
      draft.notes = result.notes.filter((note, index) => {
        if (note.start >= end) return false;
        kept.set(index, kept.size);
        return true;
      });
      memory.notes = new Set([...result.chosen].flatMap((index) => (kept.has(index) ? [kept.get(index)!] : [])));
    });
}

export interface EditorProps {
  clip: Accessor<Frozen<Clip> | undefined>;
  title: Accessor<string>;
  memory: Memory<EditorMemory>;
  /** Changes the clip as one undo step `label`. */
  edit: (label: string, change: (clip: Clip) => void) => void;
  /** Sounds a pitch on the clip's track, as a touched note. */
  preview: (pitch: number) => void;
  openMenu: (items: readonly MenuItem[]) => Promise<number | null>;
  onClose: () => void;
  height: number;
}

/** From the earliest start of `notes` to their latest end, and their earliest start. */
function spanOf(notes: readonly Note[]) {
  const start = Math.min(...notes.map((note) => note.start));
  return { start, span: Math.max(...notes.map((note) => note.start + note.length)) - start };
}

/**
 * What a key does to the chosen notes of `clip`, which starts at `clip.start` in the song with
 * the playhead at `playhead`; whether it did something. A paste goes to the playhead when it is
 * inside the clip, else right after the chosen notes, else to the start of the clip.
 */
export function editorKey(key: Key, clip: Frozen<Clip> | undefined, playhead: number, memory: Memory<EditorMemory>, edit: EditorProps["edit"]): boolean {
  if (!clip || !key.down) return false;
  // An undo may have taken chosen notes away.
  const chosen = new Set([...memory.notes].filter((index) => index < clip.notes.length));
  const change = changer(memory, edit);
  const picked = () => [...chosen].map((index) => clip.notes[index]!);
  if (key.cmd && key.key === "a") {
    memory.notes = new Set(clip.notes.map((_, index) => index));
  } else if (key.cmd && key.key === "v" && memory.copied) {
    const length = clip.length ?? 0;
    const inside = playhead >= clip.start && playhead < clip.start + length;
    const at = inside ? playhead - clip.start : chosen.size > 0 ? (({ start, span }) => start + span)(spanOf(picked())) : 0;
    const pasted = memory.copied.map((note) => ({ ...note, start: note.start + at })).filter((note) => note.start < length);
    change("Paste notes", (notes) => ({ notes: [...notes, ...pasted], chosen: new Set(pasted.map((_, index) => notes.length + index)) }));
  } else if (chosen.size === 0) {
    return false;
  } else if (key.key === "delete" || key.key === "backspace") {
    change("Delete notes", (notes) => ({ notes: notes.filter((_, index) => !chosen.has(index)), chosen: new Set() }));
  } else if (key.cmd && (key.key === "c" || key.key === "x")) {
    const { start } = spanOf(picked());
    memory.copied = picked().map((note) => ({ ...note, start: note.start - start }));
    if (key.key === "x") change("Cut notes", (notes) => ({ notes: notes.filter((_, index) => !chosen.has(index)), chosen: new Set() }));
  } else if (key.cmd && key.key === "d") {
    const { span } = spanOf(picked());
    change("Duplicate notes", (notes) => moved(notes, chosen, Math.ceil(span / BEAT) * BEAT, 0, true));
  } else if (["left", "right", "up", "down"].includes(key.key)) {
    const step = memory.snap || BEAT / 4;
    const ticks = key.key === "left" ? -step : key.key === "right" ? step : 0;
    const pitches = key.key === "up" ? (key.shift ? 12 : 1) : key.key === "down" ? (key.shift ? -12 : -1) : 0;
    change("Move notes", (notes) => moved(notes, chosen, ticks, pitches, false));
  } else if (key.key === "escape") {
    memory.notes = new Set();
  } else {
    return false;
  }
  return true;
}

/** The editor: a header with the grid and the lane, the notes, and the lane under them. */
export function ClipEditor({ clip, title, memory, edit, preview, openMenu, onClose, height }: EditorProps) {
  const isPicked = selector(() => memory.notes);
  const notes = () => clip()?.notes ?? [];
  const end = () => Math.max(clip()?.length ?? 0, ...notes().map((note) => note.start + note.length), 2 * BAR);
  const marquee = now.drag("note-marquee");
  const roll = height - 28 - LANE;

  const change = changer(memory, edit);
  // A drag is made once: the notes are drawn again with new ones when the grid changes.
  const moveDrag = (snap: number): Drag => ({
    id: "note-move",
    snap: { x: snap || 1, y: 1, grid: snap > 0 },
    onEnd: ({ dx, dy, alt }) => change(alt ? "Copy notes" : "Move notes", (all) => moved(all, memory.notes, dx, -dy, alt)),
  });
  const resizeDrag = (snap: number): Drag => ({
    id: "note-resize",
    axis: "x",
    snap: { x: snap || 1, grid: snap > 0 },
    onEnd: ({ dx }) =>
      change("Resize notes", (all) => {
        const changed = all.map((note, index) => (memory.notes.has(index) ? { ...note, length: Math.max(note.length + dx, BEAT / 16) } : note));
        const last = [...memory.notes].at(-1);
        if (last !== undefined && changed[last]) memory.length = changed[last].length;
        return { notes: changed, chosen: memory.notes };
      }),
  });
  const velocityDrag: Drag = {
    id: "note-velocity",
    axis: "y",
    onEnd: ({ dy }) =>
      change("Change velocity", (all) => ({
        notes: all.map((note, index) => (memory.notes.has(index) ? { ...note, velocity: Math.round(clamp(note.velocity - dy * 127, 1, 127)) } : note)),
        chosen: memory.notes,
      })),
  };
  const marqueeDrag: Drag = {
    id: "note-marquee",
    onEnd: ({ startX, startY, x, y, shift }) => {
      const [left, right] = [Math.min(startX, x), Math.max(startX, x)];
      const [top, bottom] = [Math.min(startY, y), Math.max(startY, y)];
      const inside = notes().flatMap((note, index) => {
        const row = rowOf(note.pitch);
        return note.start < right && note.start + note.length > left && row < bottom && row + 1 > top ? [index] : [];
      });
      memory.notes = new Set([...(shift ? memory.notes : []), ...inside]);
    },
  };
  const pressNote = (index: Accessor<number>, pitch: Accessor<number>) => ({ clicks, shift }: Press) => {
    if (clicks === 2) {
      change("Remove a note", (all) => ({ notes: all.filter((_, each) => each !== index()), chosen: new Set() }));
      return;
    }
    preview(pitch());
    if (shift) {
      const next = new Set(memory.notes);
      if (!next.delete(index())) next.add(index());
      memory.notes = next;
    } else if (!memory.notes.has(index())) {
      memory.notes = new Set([index()]);
    }
  };
  const pressEmpty = ({ clicks, shift, x, y }: Press) => {
    if (clicks === 2) {
      const note = { start: snapped(x, memory.snap), length: memory.length, pitch: clamp(127 - Math.floor(y), 0, 127), velocity: 100 };
      preview(note.pitch);
      change("Add a note", (all) => ({ notes: [...all, note], chosen: new Set([all.length]) }));
    } else if (!shift) {
      memory.notes = new Set();
    }
  };
  const chooseSnap = async () => {
    const picked = await openMenu(SNAPS.map(({ label, ticks }) => ({ label, checked: ticks === memory.snap })));
    if (picked !== null) memory.snap = SNAPS[picked]!.ticks;
  };

  // A chosen note follows the drags, natively, until the code hears the end.
  const NoteShapes = (drags: { move: Drag; resize: Drag }) => (note: Note, index: Accessor<number>) => {
    const picked = memo(() => isPicked(index()));
    const handle = () => Math.min(90, note.length / 3);
    const onPress = pressNote(index, () => note.pitch);
    return [
      <Rect x={() => note.start} y={() => rowOf(note.pitch)} width={() => note.length} height={1} radius={2}
        moves={() => (picked() ? ["note-move"] : undefined)} grows={() => (picked() ? ["note-resize"] : undefined)}
        color={() => (picked() ? COLOR.chosen : COLOR.note)} hover={() => (picked() ? undefined : COLOR.hover)} cursor="grab" drag={drags.move} onPress={onPress} />,
      <Rect x={() => note.start + note.length - handle()} y={() => rowOf(note.pitch)} width={handle} height={1}
        moves={() => (picked() ? ["note-move", "note-resize"] : undefined)}
        color={COLOR.clear} cursor="ew-resize" drag={drags.resize} onPress={onPress} anchor={() => ({ x: note.start + note.length })} />,
    ];
  };
  const VelocityBar = (note: Note, index: Accessor<number>) => {
    const picked = memo(() => isPicked(index()));
    return (
      <Rect x={() => note.start} y={() => 1 - note.velocity / 127} width={() => Math.min(note.length, 120)} height={() => note.velocity / 127}
        moves={() => (picked() ? ["note-velocity"] : undefined)} color={() => (picked() ? COLOR.chosen : COLOR.note)}
        cursor="ns-resize" drag={velocityDrag} onPress={pressNote(index, () => note.pitch)} />
    );
  };

  /** The points of an expression lane: a double click adds one, alt and a press takes one away, a drag moves one. */
  const ExpressionLane = ({ lane }: { lane: Exclude<Lane, "velocity"> }) => {
    const points = () => {
      const shown = clip();
      return shown ? pointsOf(shown, lane) : [];
    };
    // The point the pointer pressed last, which a drag moves.
    const [pressed, press] = signal<number | null>(null);
    const editPoints = (label: string, next: (points: Point[]) => Point[]) => edit(label, (draft) => setPoints(draft, lane, next(pointsOf(draft, lane))));
    const pointDrag: Drag = {
      id: "lane-point",
      onEnd: ({ dx, dy }) =>
        editPoints("Move a point", (all) =>
          all.map((point, each) => (each === pressed() ? { at: Math.max(snapped(point.at + dx, memory.snap), 0), value: valueAt(lane, upOf(lane, point.value) - dy) } : point)),
        ),
    };
    return (
      <>
        <For each={points}>
          {(point, index) => {
            const next = () => points()[index() + 1];
            return [
              <Line from={() => [point.at, 1 - upOf(lane, point.value)] as const} to={() => [next()?.at ?? end(), 1 - upOf(lane, next()?.value ?? point.value)] as const}
                color={COLOR.note} width={2} />,
              <Rect x={() => point.at - 30} y={() => 1 - upOf(lane, point.value) - 0.06} width={60} height={0.12} radius={2} color={COLOR.chosen} cursor="grab"
                drag={pointDrag} moves={() => (pressed() === index() ? ["lane-point"] : undefined)}
                onPress={({ alt }) => (alt ? editPoints("Remove a point", (all) => all.filter((_, each) => each !== index())) : press(index()))} />,
            ];
          }}
        </For>
        <Label x={() => (points()[0]?.at ?? 0) + 40} y={0.04} text={() => (points().length === 0 ? "Double-click to add a point" : "")} size={9} color={COLOR.text} />
      </>
    );
  };
  const pressLane = ({ clicks, x, y }: Press) => {
    const lane = memory.lane;
    if (clicks !== 2 || lane === "velocity") return;
    edit("Add a point", (draft) => setPoints(draft, lane, [...pointsOf(draft, lane), { at: Math.max(snapped(x, memory.snap), 0), value: valueAt(lane, 1 - y) }]));
  };

  const pitches = Array.from({ length: 128 }, (_, row) => 127 - row);
  const bars = () => Array.from({ length: Math.ceil(end() / BAR) + 1 }, (_, bar) => bar * BAR);
  const beats = () => Array.from({ length: Math.ceil(end() / BEAT) }, (_, beat) => beat * BEAT).filter((tick) => tick % BAR !== 0);
  const across = { unit: { x: ACROSS }, extent: () => ({ x: [0, end()] as const }), zoom: { x: [0.1, 8] as const } };
  const button = (on: boolean) => ({ paddingX: 6, paddingY: 2, radius: 4, fontSize: 10, background: on ? COLOR.on : COLOR.off, color: on ? COLOR.back : COLOR.white });

  return (
    <div style={{ height, gap: 2, paddingX: 8 }}>
      <div style={{ height: 24, direction: "row", gap: 8, align: "center" }}>
        <div style={{ fontSize: 11, color: COLOR.text }}>{title}</div>
        <div key="snap" style={button(false)} onPress={chooseSnap}>{() => `Snap ${SNAPS.find(({ ticks }) => ticks === memory.snap)?.label ?? "Off"}`}</div>
        {LANES.map((lane) => (
          <div key={`lane-${lane}`} style={() => button(memory.lane === lane)} onPress={() => (memory.lane = lane)}>{LANE_NAMES[lane]}</div>
        ))}
        <div key="close-editor" style={{ fontSize: 11, color: COLOR.on }} onPress={onClose}>Close</div>
      </div>
      <div style={{ direction: "row" }}>
        <Space key="editor-keys" width={KEYS} height={roll} unit={{ y: ROW }} extent={{ y: [0, 128] }} scroll={{ y: 127 - 84 }} follow={{ y: "editor" }}>
          {pitches.map((pitch) => (
            <Rect x={0} y={rowOf(pitch) + 0.04} width={isBlack(pitch) ? 22 : KEYS} height={0.92} color={isBlack(pitch) ? COLOR.black : COLOR.white}
              onPress={() => preview(pitch)} />
          ))}
          {pitches.filter((pitch) => pitch % 12 === 0).map((pitch) => (
            <Label x={KEYS - 3} y={rowOf(pitch) + 0.05} text={`C${pitch / 12 - 1}`} size={7} color={COLOR.back} align="right" />
          ))}
        </Space>
        <Space key="editor" grow height={roll} background={COLOR.back} unit={{ x: ACROSS, y: ROW }} extent={() => ({ x: [0, end()], y: [0, 128] })}
          zoom={{ x: [0.1, 8] }} scroll={{ y: 127 - 84 }} drag={marqueeDrag} onPress={pressEmpty}>
          {pitches.filter(isBlack).map((pitch) => <Rect x={0} y={rowOf(pitch)} width={1e6} height={1} color={COLOR.dark} />)}
          <For each={beats}>{(tick) => <Line from={[tick, 0]} to={[tick, 128]} color={COLOR.beat} width={1} />}</For>
          <For each={bars}>{(tick) => <Line from={[tick, 0]} to={[tick, 128]} color={COLOR.bar} width={1} />}</For>
          <Rect x={() => clip()?.length ?? end()} y={0} width={1e6} height={128} color="#00000055" />
          {() => {
            const snap = memory.snap;
            return <For each={notes}>{NoteShapes({ move: moveDrag(snap), resize: resizeDrag(snap) })}</For>;
          }}
          <Rect x={marquee.startX.min(marquee.x)} y={marquee.startY.min(marquee.y)} width={marquee.dx.abs()} height={marquee.dy.abs()}
            color={COLOR.marquee} opacity={marquee.active} />
        </Space>
      </div>
      <div style={{ direction: "row" }}>
        <div style={{ width: KEYS, fontSize: 9, color: COLOR.text, paddingY: 4 }}>{() => LANE_NAMES[memory.lane]}</div>
        <Space key="editor-lane" grow height={LANE} background={COLOR.dark} {...across} unit={{ x: ACROSS, y: LANE }} follow={{ x: "editor" }} onPress={pressLane}>
          {() => {
            const lane = memory.lane;
            return lane === "velocity" ? <For each={notes}>{VelocityBar}</For> : <ExpressionLane lane={lane} />;
          }}
        </Space>
      </div>
    </div>
  );
}
