import { blend, Button, For, Fragment, h, humanize, Label, legato, Line, memo, now, quantize, Rect, Select, selector, signal, Space, Text, untrack, type Accessor, type Color, type Context, type Drag, type FilledStateOfTool, type Frozen, type Memory, type Note, type Press } from "./sdk";

// The note editor of one timeline, as the window drew it: a header with its track, its title and a ruler,
// its notes on a grid of pitches, and under them a lane of their velocities or of one of its
// expression lanes. Times are ticks from the clip's start.
const BEAT = 960;
// Rows of 12 points, keys 40 wide at the right of a header 176 wide, a ruler 32 tall and a lane 56.
const ROW = 12;
const HEADER = 176;
const KEYS = 40;
const RULER = 32;
const LANE = 56;
// Points per tick at zoom 1, 24 a beat; it opens fitted to the clip, up to 192 a beat.
const ACROSS = 0.025;
// How far alt-up and alt-down move the chosen velocities.
const VELOCITY_STEP = 10;
// The top of a velocity of 127, and the bottom of every one, in the lane.
const [VELOCITY_TOP, VELOCITY_BOTTOM] = [8, LANE - 4];
/** The grids a note snaps to, as the arrangement's menu: a bar is as long as the bar the clip starts in. */
export const snapsOf = (bar: number) => [
  { label: "Off", ticks: 0 },
  { label: "Bar", ticks: bar },
  { label: "Beat", ticks: BEAT },
  { label: "1/8", ticks: BEAT / 2 },
  { label: "1/16", ticks: BEAT / 4 },
  { label: "1/32", ticks: BEAT / 8 },
];
const LANES = ["velocity", "pedal", "bend", "mod_wheel", "pressure"] as const;
const LANE_NAMES: Record<(typeof LANES)[number], string> = { velocity: "Velocity", pedal: "Pedal", bend: "Bend", mod_wheel: "Mod wheel", pressure: "Pressure" };
const LANE_OPTIONS = LANES.map((lane) => ({ value: lane as Lane, label: LANE_NAMES[lane] }));
/** The MIDI controllers a lane may move: every one but the mod wheel (1) and the pedal (64), which have lanes of their own. */
const CONTROLLERS = Array.from({ length: 120 }, (_, number) => number).filter((number) => number !== 1 && number !== 64);
/** What MIDI calls the controllers that have a name. */
const CONTROLLER_NAMES: Record<number, string> = {
  2: "Breath", 4: "Foot", 5: "Portamento time", 7: "Volume", 8: "Balance", 10: "Pan", 11: "Expression", 65: "Portamento", 66: "Sostenuto",
  67: "Soft pedal", 71: "Resonance", 72: "Release", 73: "Attack", 74: "Brightness", 91: "Reverb", 93: "Chorus",
};
const controllerOption = (number: number) => {
  const name = CONTROLLER_NAMES[number];
  return name ? { value: number, label: name, detail: `Controller ${number}` } : { value: number, label: `Controller ${number}` };
};
// How far Humanize moves a start and a velocity, either way.
const HUMANIZE = { timing: 20, velocity: 10 };

/** A lane the editor shows: the velocities, the pedal, an expression lane, or a MIDI controller by number. */
export type Lane = (typeof LANES)[number] | number;
export type Clip = FilledStateOfTool<"timeline">;
/** What the editor keeps in the song's memory: the chosen notes by `noteKey`, the grid, the lane shown, the last note's length and the copied notes, from 0. */
export interface EditorMemory {
  notes: Set<string>;
  snap: number;
  lane: Lane;
  length: number;
  copied: Note[] | null;
  /** How far Quantize moves a note, 0 to 1, and how much it swings, 0 to 1. */
  strength: number;
  swing: number;
  /** How many times Humanize ran, its seed. */
  humanized: number;
}

const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);
const isBlack = (pitch: number) => [1, 3, 6, 8, 10].includes(pitch % 12);
// The keys of a piano: C to E share 5 rows and F to B share 7, so white keys are as wide as on a
// real keyboard, and the black keys lie over them.
const BLACK_KEY = 24;
const WHITE_KEYS = Array.from({ length: 11 }, (_, octave) => {
  const lowKeys = [0, 2, 4].map((step, index) => ({ step, low: (index * 5) / 3, high: ((index + 1) * 5) / 3 }));
  const highKeys = [5, 7, 9, 11].map((step, index) => ({ step, low: 5 + (index * 7) / 4, high: 5 + ((index + 1) * 7) / 4 }));
  return [...lowKeys, ...highKeys].map(({ step, low, high }) => ({ pitch: octave * 12 + step, low: octave * 12 + low, high: Math.min(octave * 12 + high, 128) }));
})
  .flat()
  .filter(({ pitch }) => pitch < 128);
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
type PointLane = Exclude<Lane, "velocity">;
const limitsOf = (lane: PointLane): [number, number] => (lane === "bend" ? [-8192, 8191] : [0, 127]);
function pointsOf(clip: Frozen<Clip>, lane: PointLane): Point[] {
  const points = lane === "pedal" ? clip.pedal.map(({ start, value }) => ({ tick: start, value })) : typeof lane === "number" ? (clip.controllers[lane] ?? []) : clip[lane];
  return points.map(({ tick, value }) => ({ at: tick, value }));
}
function setPoints(clip: Clip, lane: PointLane, points: Point[]) {
  // One point per tick, in order, as the record needs them.
  const sorted = [...new Map(points.map((point) => [point.at, point])).values()].sort((a, b) => a.at - b.at);
  const ticked = sorted.map(({ at, value }) => ({ tick: at, value: Math.round(value) }));
  if (lane === "pedal") clip.pedal = ticked.map(({ tick, value }) => ({ start: tick, value }));
  else if (typeof lane !== "number") clip[lane] = ticked;
  else if (ticked.length > 0) clip.controllers = { ...clip.controllers, [lane]: ticked };
  else clip.controllers = Object.fromEntries(Object.entries(clip.controllers).filter(([number]) => Number(number) !== lane));
}
/** Where a value sits up a lane, 0 to 1, and back. */
const upOf = (lane: PointLane, value: number) => {
  const [low, high] = limitsOf(lane);
  return (value - low) / (high - low);
};
const valueAt = (lane: PointLane, up: number) => {
  const [low, high] = limitsOf(lane);
  return low + clamp(up, 0, 1) * (high - low);
};

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

/** What the note menu and Q do: to the chosen notes, or to all of them while none is. */
export type Operation = "quantize" | "humanize" | "legato";
const OPERATION_LABELS: Record<Operation, string> = { quantize: "Quantize notes", humanize: "Humanize notes", legato: "Make notes legato" };

/**
 * Quantizes the notes the editor works on to its snap (1/16 while it is off) at its strength and
 * swing, humanizes or makes them legato, as one undo step. They stay chosen and inside the clip.
 */
export function operate(operation: Operation, clip: Frozen<Clip>, memory: Memory<EditorMemory>, edit: EditorProps["edit"]) {
  const seed = operation === "humanize" ? ++memory.humanized : 0;
  const last = (clip.length ?? Infinity) - 1;
  changer(memory, edit)(OPERATION_LABELS[operation], (notes, chosen) => {
    const places = chosen.size > 0 ? [...chosen] : notes.map((_, index) => index);
    const picked = places.map((index) => notes[index]!);
    const done = operation === "quantize" ? quantize(picked, { grid: memory.snap || BEAT / 4, strength: memory.strength, swing: memory.swing })
      : operation === "humanize" ? humanize(picked, { seed, ...HUMANIZE }) : legato(picked);
    const next = [...notes];
    places.forEach((index, at) => (next[index] = { ...done[at]!, start: Math.min(done[at]!.start, last) }));
    return { notes: next, chosen };
  });
}

const STRENGTHS = [1, 0.75, 0.5];
const SWINGS = [{ label: "No swing", swing: 0 }, { label: "Light", swing: 1 / 3 }, { label: "Medium", swing: 2 / 3 }, { label: "Full", swing: 1 }];

/** The menu of the notes: Quantize with its strength and swing, Humanize and Legato. */
async function noteMenu(clip: Frozen<Clip>, memory: Memory<EditorMemory>, edit: EditorProps["edit"], openMenu: Context<unknown>["openMenu"]) {
  const items = [
    { label: "Quantize", shortcut: "song.quantize_notes" as const, run: () => operate("quantize", clip, memory, edit) },
    ...STRENGTHS.map((strength) => ({ label: `${strength * 100}%`, group: "Strength", checked: memory.strength === strength, run: () => (memory.strength = strength) })),
    ...SWINGS.map(({ label, swing }) => ({ label, group: "Swing", checked: memory.swing === swing, run: () => (memory.swing = swing) })),
    { label: "Humanize", separator: true, run: () => operate("humanize", clip, memory, edit) },
    { label: "Legato", run: () => operate("legato", clip, memory, edit) },
  ];
  const picked = await openMenu(items.map(({ run: _, ...item }) => item), { width: 200 });
  if (picked !== null) items[picked]?.run();
}

export interface EditorProps {
  clip: Accessor<Frozen<Clip> | undefined>;
  /** Where the bars of the song start, in ticks from the clip's start. */
  bars: Accessor<readonly number[]>;
  /** The track's name and colour, and the title the composer gave the clip, or none. */
  name: Accessor<string>;
  title: Accessor<string>;
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
  onClose: () => void;
  height: number;
  openMenu: Context<unknown>["openMenu"];
  /** Whether what plays the track hears any MIDI controller, as a plugin does: the lane picker offers them all then. */
  hearsControllers: Accessor<boolean>;
}

/** From the earliest start of `notes` to their latest end, and their earliest start. */
function spanOf(notes: readonly Note[]) {
  const start = Math.min(...notes.map((note) => note.start));
  return { start, span: Math.max(...notes.map((note) => note.start + note.length)) - start };
}

/** What an action of the song does to the notes of the open clip. */
export type NoteCommand = "all" | "paste" | "quantize" | "delete" | "copy" | "cut" | "duplicate" | "louder" | "softer" | "none" | keyof typeof NOTE_MOVES;
/** How far each move takes the chosen notes: in steps of the grid, and in pitches. */
const NOTE_MOVES = { left: [-1, 0], right: [1, 0], up: [0, 1], down: [0, -1], "octave-up": [0, 12], "octave-down": [0, -12] } as const;

/**
 * Does `command` to the notes of `clip`, which starts at `clip.start` in the song with the
 * playhead at `playhead`; whether there was something to do: all but choosing them all and a
 * paste need chosen notes. A paste goes to the playhead when it is inside the clip, else right
 * after the chosen notes, else to the start of the clip.
 */
export function noteCommand(command: NoteCommand, clip: Frozen<Clip>, playhead: number, memory: Memory<EditorMemory>, edit: EditorProps["edit"]): boolean {
  // Notes an undo or another tool took away are no longer chosen.
  const chosen = indexesOf(clip.notes, memory.notes);
  const change = changer(memory, edit);
  const picked = () => [...chosen].map((index) => clip.notes[index]!);
  const without = (notes: Note[], gone: ReadonlySet<number>) => ({ notes: notes.filter((_, index) => !gone.has(index)), chosen: new Set<number>() });
  if (command === "all") {
    memory.notes = new Set(clip.notes.map(noteKey));
  } else if (command === "paste") {
    if (!memory.copied) return false;
    const inside = playhead >= clip.start && playhead < clip.start + lengthOf(clip);
    const at = inside ? playhead - clip.start : chosen.size > 0 ? (({ start, span }) => start + span)(spanOf(picked())) : 0;
    const pasted = memory.copied.map((note) => ({ ...note, start: note.start + at }));
    change("Paste notes", (notes) => ({ notes: [...notes, ...pasted], chosen: new Set(pasted.map((_, index) => notes.length + index)) }));
  } else if (command === "quantize") {
    operate("quantize", clip, memory, edit);
  } else if (chosen.size === 0) {
    return false;
  } else if (command === "delete") {
    change("Delete notes", without);
  } else if (command === "copy" || command === "cut") {
    const { start } = spanOf(picked());
    memory.copied = picked().map((note) => ({ ...note, start: note.start - start }));
    if (command === "cut") change("Cut notes", without);
  } else if (command === "duplicate") {
    const { span } = spanOf(picked());
    change("Duplicate notes", (notes, all) => moved(notes, all, span, 0, true));
  } else if (command === "louder" || command === "softer") {
    const by = command === "louder" ? VELOCITY_STEP : -VELOCITY_STEP;
    change("Change velocity", (notes, all) => ({ notes: notes.map((note, index) => (all.has(index) ? { ...note, velocity: clamp(note.velocity + by, 1, 127) } : note)), chosen: all }));
  } else if (command === "none") {
    memory.notes = new Set();
  } else {
    const [steps, pitches] = NOTE_MOVES[command];
    change("Move notes", (notes, all) => moved(notes, all, steps * (memory.snap || BEAT / 4), pitches, false));
  }
  return true;
}

/** The editor: a header with the track, the clip's title, the ruler and its buttons, the notes, and the lane under them. */
export function ClipEditor({ clip, bars, name, title, colour, id, memory, edit, preview, seek, onClose, height, openMenu, hearsControllers }: EditorProps) {
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
    onEnd: ({ dx, dy, alt }) => change(alt ? "Copy notes" : "Move notes", (all, chosen) => moved(all, chosen, Math.round(dx), -dy, alt)),
  });
  const resizeDrag = (snap: number): Drag => ({
    id: "note-resize",
    axis: "x",
    snap: { x: snap || 1, grid: snap > 0 },
    onEnd: ({ dx }) =>
      change("Resize notes", (all, chosen) => {
        const changed = all.map((note, index) => (chosen.has(index) ? { ...note, length: Math.max(note.length + Math.round(dx), BEAT / 16) } : note));
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
  const openNoteMenu = () => {
    const shown = clip();
    if (shown) void noteMenu(shown, memory, edit, openMenu);
  };
  const pressNote = (note: Note) => ({ shift, cmd, button }: Press) => {
    if (button === "right") {
      if (!memory.notes.has(noteKey(note))) memory.notes = new Set([noteKey(note)]);
      openNoteMenu();
      return;
    }
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
  const pressEmpty = ({ clicks, shift, cmd, x, y, button }: Press) => {
    if (button === "right") {
      openNoteMenu();
    } else if (clicks === 2) {
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

  /** 1 while the rectangle being dragged touches `note`, as it chooses it when let go. */
  const boxed = (note: Note) => {
    const row = rowOf(note.pitch);
    return marquee.active
      .times(marquee.startX.min(marquee.x).lt(note.start + note.length))
      .times(marquee.startX.max(marquee.x).gt(note.start))
      .times(marquee.startY.min(marquee.y).lt(row + 1))
      .times(marquee.startY.max(marquee.y).gt(row));
  };
  // A chosen note follows the drags, natively, until the code hears the end.
  const NoteShapes = (drags: { move: Drag; resize: Drag }) => (note: Note) => {
    const picked = memo(() => isPicked(noteKey(note)));
    const handle = () => Math.min(90, note.length / 3);
    const onPress = pressNote(note);
    // A plain click on one of several chosen notes chooses it alone.
    const onClick = ({ shift, cmd }: Press) => {
      if (!shift && !cmd) memory.notes = new Set([noteKey(note)]);
    };
    // A point short of its length and of its row, as the window drew notes apart.
    return [
      <Rect x={() => note.start} y={() => rowOf(note.pitch)} width={() => space.pointX.negate().plus(note.length).max(space.pointX.times(2))} height={(ROW - 1) / ROW} radius={3}
        moves={() => (picked() ? ["note-move"] : undefined)} grows={() => (picked() ? ["note-resize"] : undefined)}
        color={() => (picked() ? "text" : blend(colour(), "text", boxed(note)))} border={colour} borderWidth={1} cursor="grab" drag={drags.move} onPress={onPress} onClick={onClick} />,
      <Rect x={() => note.start + note.length - handle()} y={() => rowOf(note.pitch)} width={handle} height={1}
        moves={() => (picked() ? ["note-move", "note-resize"] : undefined)}
        color="none" cursor="ew-resize" drag={drags.resize} onPress={onPress} anchor={() => ({ x: note.start + note.length })} />,
    ];
  };
  const VelocityBar = (note: Note) => {
    const picked = memo(() => isPicked(noteKey(note)));
    const top = () => (VELOCITY_BOTTOM - ((VELOCITY_BOTTOM - VELOCITY_TOP) * note.velocity) / 127) / LANE;
    return (
      <Rect x={() => note.start} y={top} width={space.pointX.times(3)} height={() => VELOCITY_BOTTOM / LANE - top()}
        moves={() => (picked() ? ["note-velocity"] : undefined)} color={() => (picked() ? "text" : blend(colour(), "text", boxed(note)))}
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
              <Rect x={() => space.pointX.times(-4).plus(point.at)} y={() => 1 - upOf(lane, point.value) - 4 / LANE} width={space.pointX.times(8)} height={8 / LANE} radius={4} color="text" cursor="grab"
                drag={pointDrag} moves={() => (pressed() === index() ? ["lane-point"] : undefined)}
                onPress={({ alt }) => (alt ? editPoints("Remove a point", (all) => all.filter((_, each) => each !== index())) : press(index()))} />,
            ];
          }}
        </For>
        <Label x={() => space.pointX.times(8).plus(points()[0]?.at ?? 0)} y={0.08} text={() => (points().length === 0 ? "Double-click to add a point" : "")} color="text-muted" />
      </>
    );
  };
  const pressLane = ({ clicks, x, y }: Press) => {
    const lane = memory.lane;
    if (clicks !== 2 || lane === "velocity") return;
    edit("Add a point", (draft) => setPoints(draft, lane, [...pointsOf(draft, lane), { at: Math.max(snapped(x, memory.snap), 0), value: valueAt(lane, 1 - y) }]));
  };

  // The lanes, then the clip's own controllers, then every other one where the track hears them.
  const laneOptions = () => {
    const own = Object.keys(clip()?.controllers ?? {}).map(Number).sort((a, b) => a - b);
    const others = hearsControllers() ? CONTROLLERS : typeof memory.lane === "number" ? [memory.lane] : [];
    return [...LANE_OPTIONS, ...[...own, ...others.filter((number) => !own.includes(number))].map(controllerOption)];
  };
  const pitches = Array.from({ length: 128 }, (_, row) => 127 - row);
  const lines = () => bars().filter((tick) => tick >= -BEAT && tick <= end());
  const beats = () => {
    const starts = new Set(lines());
    return Array.from({ length: Math.ceil(end() / BEAT) }, (_, beat) => beat * BEAT).filter((tick) => !starts.has(tick));
  };
  // The roll and what follows it across: the ruler and the lane.
  const across = { unit: { x: ACROSS }, extent: () => ({ x: [-BEAT, end()] as const }), zoom: { x: [0.1, 8] as const } };
  const Rule = () => <div style={{ width: 1, background: "border", keep: true }} />;

  return (
    <div style={{ height, keep: true }}>
      <div style={{ height: RULER, direction: "row", keep: true }}>
        <div style={{ width: HEADER - 1, direction: "row", align: "center", gap: "sm", paddingLeft: "lg", paddingRight: "sm", keep: true }}>
          <div style={() => ({ width: 8, height: 8, radius: "full", keep: true, background: colour() })} />
          {/* A clip the composer named leads with its title, and its track's name takes the room that is left. */}
          {() => title() ? (
            <>
              <Text variant="heading" style={{ maxWidth: 96, truncate: true, keep: true }}>{title}</Text>
              <Text variant="label" style={{ grow: true, truncate: true }}>{name}</Text>
            </>
          ) : <Text variant="heading" style={{ grow: true, truncate: true }}>{name}</Text>}
        </div>
        <Rule />
        <Space key={`${key}-ruler`} grow height={RULER} {...across} follow={{ x: key }}
          onPress={({ x }) => seek(Math.max(memory.snap > 0 ? Math.round(x / memory.snap) * memory.snap : Math.round(x), 0))}>
          <For each={() => bars().map((tick, index) => ({ tick, index }))}>
            {({ tick, index }) => [
              <Rect x={tick} y={RULER - 8} width={space.pointX} height={8} color="border-strong" />,
              <Label x={space.pointX.times(8).plus(tick)} y={8} text={String(index + 1)} color="text-muted" />,
            ]}
          </For>
        </Space>
        <div style={{ direction: "row", align: "center", paddingX: "sm", keep: true }}>
          <Button key="note-menu" icon="ellipsis" variant="ghost" size="sm" tooltip="Quantize, humanize, legato" onClick={openNoteMenu} />
          <Button key="close-editor" icon="x" variant="ghost" size="sm" tooltip="Close" shortcut="song.back" onClick={onClose} />
        </div>
      </div>
      <div style={{ height: 1, background: "border", keep: true }} />
      <div style={{ height: roll, direction: "row", keep: true }}>
        <Space key={`${key}-keys`} width={HEADER - 1} height={roll} unit={{ y: ROW }} extent={{ y: [0, 128] }} scroll={{ y: middleRow }} follow={{ y: key }}>
          {WHITE_KEYS.map(({ pitch, low, high }) => (
            <Rect x={HEADER - 1 - KEYS} y={128 - high + 0.5 / ROW} width={KEYS} height={high - low - 1 / ROW} radius={2} color="fill-strong"
              onPress={() => preview(pitch)} />
          ))}
          {pitches.filter(isBlack).map((pitch) => (
            <Rect x={HEADER - 1 - KEYS} y={rowOf(pitch) + 1 / ROW} width={BLACK_KEY} height={(ROW - 2) / ROW} radius={2} color="well"
              onPress={() => preview(pitch)} />
          ))}
          {pitches.filter((pitch) => pitch % 12 === 0).map((pitch) => (
            <Label x={HEADER - 1 - KEYS - 8} y={rowOf(pitch)} text={`C${pitch / 12 - 1}`} color="text-muted" align="right" />
          ))}
        </Space>
        <Rule />
        <Space key={key} grow height={roll} {...across} unit={{ x: ACROSS, y: ROW }} extent={() => ({ x: [-BEAT, end()] as const, y: [0, 128] as const })}
          fit={{ x: [0, opening], before: 8, after: 96 }} scroll={{ y: middleRow }} drag={marqueeDrag} onPress={pressEmpty}>
          {pitches.filter(isBlack).map((pitch) => <Rect x={-1e6} y={rowOf(pitch)} width={2e6} height={1} color="fill" opacity={0.4} />)}
          <For each={beats}>{(tick) => <Line from={[tick, 0]} to={[tick, 128]} color="border" width={1} />}</For>
          <For each={lines}>{(tick) => <Line from={[tick, 0]} to={[tick, 128]} color="border-strong" width={1} />}</For>
          <Rect x={length} y={0} width={1e6} height={128} color="background" />
          <Rect x={-1e6} y={0} width={1e6} height={128} color="background" />
          {() => {
            const snap = memory.snap;
            return <For each={notes}>{NoteShapes({ move: moveDrag(snap), resize: resizeDrag(snap) })}</For>;
          }}
          <Rect x={marquee.startX.min(marquee.x)} y={marquee.startY.min(marquee.y)} width={marquee.dx.abs()} height={marquee.dy.abs()}
            color="fill" border="border-strong" borderWidth={1} radius={2} opacity={marquee.active} />
        </Space>
      </div>
      <div style={{ height: 1, background: "border", keep: true }} />
      <div style={{ height: LANE, direction: "row", keep: true }}>
        <div style={{ width: HEADER - 1, paddingLeft: "lg", justify: "center", align: "start", keep: true }}>
          <Select<Lane> key="editor-lane-pick" size="sm" title="Lane" value={() => memory.lane} options={laneOptions} search menuWidth={220}
            onChange={(lane) => (memory.lane = lane)} />
        </div>
        <Rule />
        <Space key={`${key}-lane`} grow height={LANE} {...across} unit={{ x: ACROSS, y: LANE }} follow={{ x: key }} onPress={pressLane} drag={velocityDraw}>
          {() => {
            const lane = memory.lane;
            return lane === "velocity" ? <For each={notes}>{VelocityBar}</For> : <ExpressionLane lane={lane} />;
          }}
          <Line from={[drawn.startX, drawn.startY]} to={[drawn.startX.plus(drawn.dx.times(drawn.active)), drawn.startY.plus(drawn.dy.times(drawn.active))]} color="text" width={1} />
        </Space>
      </div>
    </div>
  );
}
