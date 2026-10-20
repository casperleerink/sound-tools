import { Button, For, Fragment, h, Icon, Label, Line, memo, now, Rect, selector, signal, Space, untrack, type Accessor, type Color, type Drag, type FilledStateOfTool, type Frozen, type Key, type Memory, type MenuItem, type Note, type Press } from "./sdk";

// The note editor of one timeline, as the window drew it: a header with its track and a ruler,
// its notes on a grid of pitches, and under them a lane of their velocities or of one of its
// expression lanes. Times are ticks from the clip's start.
const BEAT = 960;
// Rows of 12 points, keys 32 wide at the right of a header 176 wide, a ruler 32 tall and a lane 56.
const ROW = 12;
const HEADER = 176;
const KEYS = 32;
const RULER = 32;
const LANE = 56;
// Points per tick at zoom 1, 24 a beat; it opens fitted to the clip, up to 192 a beat.
const ACROSS = 0.025;
// How far alt-up and alt-down move the chosen velocities.
const VELOCITY_STEP = 10;
// The top of a velocity of 127, and the bottom of every one, in the lane.
const [VELOCITY_TOP, VELOCITY_BOTTOM] = [8, LANE - 4];
/** The grids a note snaps to: a bar is as long as the bar the clip starts in. */
export const snapsOf = (bar: number) => [
  { label: "Bar", ticks: bar },
  { label: "1/4", ticks: BEAT },
  { label: "1/8", ticks: BEAT / 2 },
  { label: "1/16", ticks: BEAT / 4 },
  { label: "1/32", ticks: BEAT / 8 },
  { label: "Off", ticks: 0 },
];
const LANES = ["velocity", "pedal", "bend", "mod_wheel", "pressure"] as const;
const LANE_NAMES: Record<Lane, string> = { velocity: "Velocity", pedal: "Pedal", bend: "Bend", mod_wheel: "Mod wheel", pressure: "Pressure" };

export type Lane = (typeof LANES)[number];
export type Clip = FilledStateOfTool<"timeline">;
/** What the editor keeps in the song's memory: the chosen notes by `noteKey`, the grid, the lane shown, the last note's length and the copied notes, from 0. */
export interface EditorMemory {
  notes: Set<string>;
  snap: number;
  lane: Lane;
  length: number;
  copied: Note[] | null;
}

const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);
const isBlack = (pitch: number) => [1, 3, 6, 8, 10].includes(pitch % 12);
const rowOf = (pitch: number) => 127 - pitch;
const snapped = (tick: number, snap: number) => (snap > 0 ? Math.floor(tick / snap) * snap : Math.round(tick));
/** A note by what it is: an undo or an edit of another tool that moves notes in the list keeps it chosen. */
const noteKey = (note: Note) => `${note.start}:${note.pitch}:${note.length}:${note.velocity}`;
/** Where the notes `chosen` names are in `notes`. */
const indexesOf = (notes: readonly Note[], chosen: ReadonlySet<string>) =>
  new Set(notes.flatMap((note, index) => (chosen.has(noteKey(note)) ? [index] : [])));
/** How long a timeline plays: its `length`, else to the end of its last note, as it plays it. */
export const lengthOf = (clip: Frozen<Clip>) => clip.length ?? Math.max(1, ...clip.notes.map((note) => note.start + note.length));

/** A point of a lane as the record holds it: the pedal counts from `start`, the others from `tick`. */
type Point = { at: number; value: number };
const LIMITS: Record<Exclude<Lane, "velocity">, [number, number]> = { pedal: [0, 127], bend: [-8192, 8191], mod_wheel: [0, 127], pressure: [0, 127] };
function pointsOf(clip: Frozen<Clip>, lane: Exclude<Lane, "velocity">): Point[] {
  return lane === "pedal" ? clip.pedal.map(({ start, value }) => ({ at: start, value })) : clip[lane].map(({ tick, value }) => ({ at: tick, value }));
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
 * Changes the notes of the clip as `next` says, given the places of the chosen ones, as one undo
 * step, and chooses what it chose. A note that would start at or past the end of the clip is
 * left out: the timeline plays none there. A clip with no `length` plays to its last note.
 */
function changer(memory: Memory<EditorMemory>, edit: EditorProps["edit"]) {
  return (label: string, next: (notes: Note[], chosen: ReadonlySet<number>) => { notes: Note[]; chosen: ReadonlySet<number> }) =>
    edit(label, (draft) => {
      const result = next(draft.notes, indexesOf(draft.notes, memory.notes));
      const end = draft.length ?? Infinity;
      const kept = result.notes.filter((note) => note.start < end);
      draft.notes = kept;
      memory.notes = new Set([...result.chosen].flatMap((index) => {
        const note = result.notes[index];
        return note && note.start < end ? [noteKey(note)] : [];
      }));
    });
}

export interface EditorProps {
  clip: Accessor<Frozen<Clip> | undefined>;
  /** Where the bars of the song start, in ticks from the clip's start. */
  bars: Accessor<readonly number[]>;
  /** The track's name and colour. */
  name: Accessor<string>;
  colour: Accessor<Color>;
  /** Keeps the editor's scroll and zoom, and fits it to a clip once: the clip's `track/clip`. */
  id: string;
  memory: Memory<EditorMemory>;
  /** Changes the clip as one undo step `label`. */
  edit: (label: string, change: (clip: Clip) => void) => void;
  /** Sounds a pitch on the clip's track, as a touched note. */
  preview: (pitch: number) => void;
  /** Plays from a tick of the clip. */
  seek: (tick: number) => void;
  openMenu: (items: readonly MenuItem[], options?: { width?: number }) => Promise<number | null>;
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
  // Notes an undo or another tool took away are no longer chosen.
  const chosen = indexesOf(clip.notes, memory.notes);
  const change = changer(memory, edit);
  const picked = () => [...chosen].map((index) => clip.notes[index]!);
  const without = (notes: Note[], gone: ReadonlySet<number>) => ({ notes: notes.filter((_, index) => !gone.has(index)), chosen: new Set<number>() });
  if (key.cmd && key.key === "a") {
    memory.notes = new Set(clip.notes.map(noteKey));
  } else if (key.cmd && key.key === "v" && memory.copied) {
    const inside = playhead >= clip.start && playhead < clip.start + lengthOf(clip);
    const at = inside ? playhead - clip.start : chosen.size > 0 ? (({ start, span }) => start + span)(spanOf(picked())) : 0;
    const pasted = memory.copied.map((note) => ({ ...note, start: note.start + at }));
    change("Paste notes", (notes) => ({ notes: [...notes, ...pasted], chosen: new Set(pasted.map((_, index) => notes.length + index)) }));
  } else if (chosen.size === 0) {
    return false;
  } else if (key.key === "delete" || key.key === "backspace") {
    change("Delete notes", without);
  } else if (key.cmd && (key.key === "c" || key.key === "x")) {
    const { start } = spanOf(picked());
    memory.copied = picked().map((note) => ({ ...note, start: note.start - start }));
    if (key.key === "x") change("Cut notes", without);
  } else if (key.cmd && key.key === "d") {
    const { span } = spanOf(picked());
    change("Duplicate notes", (notes, all) => moved(notes, all, span, 0, true));
  } else if (key.alt && (key.key === "up" || key.key === "down")) {
    const by = key.key === "up" ? VELOCITY_STEP : -VELOCITY_STEP;
    change("Change velocity", (notes, all) => ({ notes: notes.map((note, index) => (all.has(index) ? { ...note, velocity: clamp(note.velocity + by, 1, 127) } : note)), chosen: all }));
  } else if (key.key === "left" || key.key === "right" || key.key === "up" || key.key === "down") {
    const step = memory.snap || BEAT / 4;
    const ticks = key.key === "left" ? -step : key.key === "right" ? step : 0;
    const pitches = key.key === "up" ? (key.shift ? 12 : 1) : key.key === "down" ? (key.shift ? -12 : -1) : 0;
    change("Move notes", (notes, all) => moved(notes, all, ticks, pitches, false));
  } else if (key.key === "escape") {
    memory.notes = new Set();
  } else {
    return false;
  }
  return true;
}

/** The editor: a header with the track and the ruler, the notes, and the lane under them. */
export function ClipEditor({ clip, bars, name, colour, id, memory, edit, preview, seek, openMenu, onClose, height }: EditorProps) {
  const isPicked = selector(() => memory.notes);
  const notes = () => clip()?.notes ?? [];
  const length = () => {
    const shown = clip();
    return shown ? lengthOf(shown) : 4 * BEAT;
  };
  const end = () => length() + 8 * BEAT;
  const key = `editor-${id}`;
  const space = now.space(key);
  const marquee = now.drag("note-marquee");
  const drawn = now.drag("velocity-draw");
  const roll = height - RULER - LANE - 2;
  // It opens on the clip: the middle of its notes in the middle of the roll, on whole points.
  const [opening, middleRow] = untrack(() => {
    const pitches = notes().map((note) => note.pitch);
    const middle = pitches.length > 0 ? (Math.min(...pitches) + Math.max(...pitches)) / 2 : 60;
    return [length(), Math.round((127 - middle + 0.5) * ROW - roll / 2) / ROW] as const;
  });

  const change = changer(memory, edit);
  // A drag is made once: the notes are drawn again with new ones when the grid changes.
  const moveDrag = (snap: number): Drag => ({
    id: "note-move",
    snap: { x: snap || 1, y: 1, grid: snap > 0 },
    onEnd: ({ dx, dy, alt }) => change(alt ? "Copy notes" : "Move notes", (all, chosen) => moved(all, chosen, dx, -dy, alt)),
    // A plain click on one of several chosen notes chooses it alone.
    onClick: ({ startX, startY, shift, cmd }) => {
      const row = Math.floor(startY);
      const note = notes().find((each) => rowOf(each.pitch) === row && each.start <= startX && each.start + each.length > startX);
      if (note && !shift && !cmd) memory.notes = new Set([noteKey(note)]);
    },
  });
  const resizeDrag = (snap: number): Drag => ({
    id: "note-resize",
    axis: "x",
    snap: { x: snap || 1, grid: snap > 0 },
    onEnd: ({ dx }) =>
      change("Resize notes", (all, chosen) => {
        const changed = all.map((note, index) => (chosen.has(index) ? { ...note, length: Math.max(note.length + dx, BEAT / 16) } : note));
        const last = [...chosen].at(-1);
        if (last !== undefined && changed[last]) memory.length = changed[last].length;
        return { notes: changed, chosen };
      }),
  });
  const velocityDrag: Drag = {
    id: "note-velocity",
    axis: "y",
    onEnd: ({ dy }) =>
      change("Change velocity", (all, chosen) => ({
        notes: all.map((note, index) => (chosen.has(index) ? { ...note, velocity: Math.round(clamp(note.velocity - (dy * 127 * LANE) / (VELOCITY_BOTTOM - VELOCITY_TOP), 1, 127)) } : note)),
        chosen,
      })),
  };
  const marqueeDrag: Drag = {
    id: "note-marquee",
    onEnd: ({ startX, startY, x, y, shift, cmd }) => {
      const [left, right] = [Math.min(startX, x), Math.max(startX, x)];
      const [top, bottom] = [Math.min(startY, y), Math.max(startY, y)];
      const inside = notes().flatMap((note) => {
        const row = rowOf(note.pitch);
        return note.start < right && note.start + note.length > left && row < bottom && row + 1 > top ? [noteKey(note)] : [];
      });
      memory.notes = new Set([...(shift || cmd ? memory.notes : []), ...inside]);
    },
  };
  const pressNote = (note: Note) => ({ shift, cmd }: Press) => {
    preview(note.pitch);
    const key = noteKey(note);
    if (shift || cmd) {
      const next = new Set(memory.notes);
      if (!next.delete(key)) next.add(key);
      memory.notes = next;
    } else if (!memory.notes.has(key)) {
      memory.notes = new Set([key]);
    }
  };
  const pressEmpty = ({ clicks, shift, cmd, x, y }: Press) => {
    if (clicks === 2) {
      const note = { start: snapped(x, memory.snap), length: memory.length, pitch: clamp(127 - Math.floor(y), 0, 127), velocity: 100 };
      preview(note.pitch);
      change("Add a note", (all) => ({ notes: [...all, note], chosen: new Set([all.length]) }));
    } else if (!shift && !cmd) {
      memory.notes = new Set();
    }
  };
  /** Across the velocity lane: each note the line passes gets the velocity of its height there. */
  const velocityDraw: Drag = {
    id: "velocity-draw",
    onEnd: ({ startX, startY, x, y }) => {
      if (memory.lane !== "velocity") return;
      const [left, right] = [Math.min(startX, x), Math.max(startX, x)];
      const heightAt = (tick: number) => (x === startX ? y : startY + ((tick - startX) * (y - startY)) / (x - startX));
      const velocityAt = (up: number) => Math.round(clamp(((VELOCITY_BOTTOM - up * LANE) * 127) / (VELOCITY_BOTTOM - VELOCITY_TOP), 1, 127));
      change("Draw velocities", (all, chosen) => ({
        notes: all.map((note) => (note.start >= left && note.start <= right ? { ...note, velocity: velocityAt(heightAt(note.start)) } : note)),
        chosen,
      }));
    },
  };
  const chooseLane = async () => {
    const picked = await openMenu(LANES.map((lane) => ({ label: LANE_NAMES[lane], group: "Lane", checked: memory.lane === lane })), { width: 160 });
    if (picked !== null) memory.lane = LANES[picked]!;
  };

  // A chosen note follows the drags, natively, until the code hears the end.
  const NoteShapes = (drags: { move: Drag; resize: Drag }) => (note: Note) => {
    const picked = memo(() => isPicked(noteKey(note)));
    const handle = () => Math.min(90, note.length / 3);
    const onPress = pressNote(note);
    // A point short of its length and of its row, as the window drew notes apart.
    return [
      <Rect x={() => note.start} y={() => rowOf(note.pitch)} width={() => space.pointX.negate().plus(note.length).max(space.pointX.times(2))} height={(ROW - 1) / ROW} radius={3}
        moves={() => (picked() ? ["note-move"] : undefined)} grows={() => (picked() ? ["note-resize"] : undefined)}
        color={() => (picked() ? "gray-950" : colour())} border={colour} borderWidth={1} cursor="grab" drag={drags.move} onPress={onPress} />,
      <Rect x={() => note.start + note.length - handle()} y={() => rowOf(note.pitch)} width={handle} height={1}
        moves={() => (picked() ? ["note-move", "note-resize"] : undefined)}
        color="#00000000" cursor="ew-resize" drag={drags.resize} onPress={onPress} anchor={() => ({ x: note.start + note.length })} />,
    ];
  };
  const VelocityBar = (note: Note) => {
    const picked = memo(() => isPicked(noteKey(note)));
    const top = () => (VELOCITY_BOTTOM - ((VELOCITY_BOTTOM - VELOCITY_TOP) * note.velocity) / 127) / LANE;
    return (
      <Rect x={() => note.start} y={top} width={space.pointX.times(3)} height={() => VELOCITY_BOTTOM / LANE - top()}
        moves={() => (picked() ? ["note-velocity"] : undefined)} color={() => (picked() ? "gray-950" : (`${colour()}/70` as Color))}
        cursor="ns-resize" drag={velocityDrag} onPress={pressNote(note)} />
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
                color={colour} width={2} />,
              <Rect x={() => space.pointX.times(-4).plus(point.at)} y={() => 1 - upOf(lane, point.value) - 4 / LANE} width={space.pointX.times(8)} height={8 / LANE} radius={4} color="gray-950" cursor="grab"
                drag={pointDrag} moves={() => (pressed() === index() ? ["lane-point"] : undefined)}
                onPress={({ alt }) => (alt ? editPoints("Remove a point", (all) => all.filter((_, each) => each !== index())) : press(index()))} />,
            ];
          }}
        </For>
        <Label x={() => space.pointX.times(8).plus(points()[0]?.at ?? 0)} y={0.08} text={() => (points().length === 0 ? "Double-click to add a point" : "")} size={11} color="gray-700" />
      </>
    );
  };
  const pressLane = ({ clicks, x, y }: Press) => {
    const lane = memory.lane;
    if (clicks !== 2 || lane === "velocity") return;
    edit("Add a point", (draft) => setPoints(draft, lane, [...pointsOf(draft, lane), { at: Math.max(snapped(x, memory.snap), 0), value: valueAt(lane, 1 - y) }]));
  };

  const pitches = Array.from({ length: 128 }, (_, row) => 127 - row);
  const lines = () => bars().filter((tick) => tick >= -BEAT && tick <= end());
  const beats = () => {
    const starts = new Set(lines());
    return Array.from({ length: Math.ceil(end() / BEAT) }, (_, beat) => beat * BEAT).filter((tick) => !starts.has(tick));
  };
  // The roll and what follows it across: the ruler and the lane.
  const across = { unit: { x: ACROSS }, extent: () => ({ x: [-BEAT, end()] as const }), zoom: { x: [1, 8] as const } };
  const Rule = () => <div style={{ width: 1, background: "alpha/5", keep: true }} />;

  return (
    <div style={{ height, keep: true }}>
      <div style={{ height: RULER, direction: "row", keep: true }}>
        <div style={{ width: HEADER - 1, direction: "row", align: "center", gap: 8, paddingLeft: 20, paddingRight: 8, keep: true }}>
          <div style={() => ({ width: 8, height: 8, radius: 4, keep: true, background: colour() })} />
          <div style={{ grow: true, fontSize: 14, weight: "medium", truncate: true, color: "gray-900" }}>{name}</div>
          <Button key="close-editor" icon="x" variant="ghost" size="xs" opacity={0.6} onPress={onClose} />
        </div>
        <Rule />
        <Space key={`${key}-ruler`} grow height={RULER} {...across} follow={{ x: key }}
          onPress={({ x }) => seek(Math.max(memory.snap > 0 ? Math.round(x / memory.snap) * memory.snap : Math.round(x), 0))}>
          <For each={() => bars().map((tick, index) => ({ tick, index }))}>
            {({ tick, index }) => [
              <Rect x={tick} y={RULER - 8} width={space.pointX} height={8} color="alpha/10" />,
              <Label x={space.pointX.times(8).plus(tick)} y={8} text={String(index + 1)} size={12} color="gray-700" />,
            ]}
          </For>
        </Space>
      </div>
      <div style={{ height: 1, background: "alpha/5", keep: true }} />
      <div style={{ height: roll, direction: "row", keep: true }}>
        <Space key={`${key}-keys`} width={HEADER - 1} height={roll} unit={{ y: ROW }} extent={{ y: [0, 128] }} scroll={{ y: middleRow }} follow={{ y: key }}>
          {pitches.map((pitch) => (
            <Rect x={HEADER - 1 - KEYS} y={rowOf(pitch) + 0.5 / ROW} width={KEYS} height={(ROW - 1) / ROW} color={isBlack(pitch) ? "alpha/3" : "alpha/10"}
              onPress={() => preview(pitch)} />
          ))}
          {pitches.filter((pitch) => pitch % 12 === 0).map((pitch) => (
            <Label x={HEADER - 1 - KEYS - 8} y={rowOf(pitch)} text={`C${pitch / 12 - 1}`} size={12} color="gray-700" align="right" />
          ))}
        </Space>
        <Rule />
        <Space key={key} grow height={roll} {...across} unit={{ x: ACROSS, y: ROW }} extent={() => ({ x: [-BEAT, end()] as const, y: [0, 128] as const })}
          fit={{ x: [0, opening], before: 8, after: 96 }} scroll={{ y: middleRow }} drag={marqueeDrag} onPress={pressEmpty}>
          {pitches.filter(isBlack).map((pitch) => <Rect x={-1e6} y={rowOf(pitch)} width={2e6} height={1} color="alpha/2" />)}
          <For each={beats}>{(tick) => <Line from={[tick, 0]} to={[tick, 128]} color="alpha/5" width={1} />}</For>
          <For each={lines}>{(tick) => <Line from={[tick, 0]} to={[tick, 128]} color="alpha/7" width={1} />}</For>
          <Rect x={length} y={0} width={1e6} height={128} color="gray-100" />
          <Rect x={-1e6} y={0} width={1e6} height={128} color="gray-100" />
          {() => {
            const snap = memory.snap;
            return <For each={notes}>{NoteShapes({ move: moveDrag(snap), resize: resizeDrag(snap) })}</For>;
          }}
          <Rect x={marquee.startX.min(marquee.x)} y={marquee.startY.min(marquee.y)} width={marquee.dx.abs()} height={marquee.dy.abs()}
            color="alpha/5" border="alpha/20" borderWidth={1} radius={2} opacity={marquee.active} />
        </Space>
      </div>
      <div style={{ height: 1, background: "alpha/5", keep: true }} />
      <div style={{ height: LANE, direction: "row", keep: true }}>
        <div style={{ width: HEADER - 1, paddingLeft: 16, justify: "center", align: "start", keep: true }}>
          <div key="editor-lane-pick" style={{ direction: "row", align: "center", gap: 6, height: 24, paddingLeft: 9, paddingRight: 8, radius: 6, background: "alpha/5", hover: "alpha/10" }}
            onPress={chooseLane}>
            <div style={{ fontSize: 13, weight: "medium", color: "gray-950" }}>{() => LANE_NAMES[memory.lane]}</div>
            <Icon name="chevron-down" size={12} color="gray-700" />
          </div>
        </div>
        <Rule />
        <Space key={`${key}-lane`} grow height={LANE} {...across} unit={{ x: ACROSS, y: LANE }} follow={{ x: key }} onPress={pressLane} drag={velocityDraw}>
          {() => {
            const lane = memory.lane;
            return lane === "velocity" ? <For each={notes}>{VelocityBar}</For> : <ExpressionLane lane={lane} />;
          }}
          <Line from={[drawn.startX, drawn.startY]} to={[drawn.startX.plus(drawn.dx.times(drawn.active)), drawn.startY.plus(drawn.dy.times(drawn.active))]} color="gray-950" width={1} />
        </Space>
      </div>
    </div>
  );
}
