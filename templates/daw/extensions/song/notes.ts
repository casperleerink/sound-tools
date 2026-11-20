// The notes of the clip the editor shows: choose, add, move, copy, resize, delete them, change
// their velocity, quantize, humanize, and the points of its expression lanes. Times are ticks
// from the clip's start.
// The roll's plane: x in ticks of the clip, y in rows down from pitch 127, `127 - pitch`.
// The velocity lane's: y is `127 - velocity`. An expression lane's: y down its travel, 0 to 1.
import { humanize, legato, quantize, type FilledStateOfTool, type Frozen, type Note, type Pointer } from "../sdk";
import { editClips, lengthOf } from "./clips";
import { itemAt, rowOfKey, type Song } from "./model";
import { barAt, BEAT, cellOf, stepAt, stepped } from "./time";

type Clip = FilledStateOfTool<"timeline">;
/** A lane under the notes: the velocities, the pedal, an expression lane, or a MIDI controller by number. */
export type NoteLane = (typeof LANES)[number] | number;
export const LANES = ["velocity", "pedal", "bend", "mod_wheel", "pressure"] as const;
export const LANE_NAMES: Record<(typeof LANES)[number], string> = { velocity: "Velocity", pedal: "Pedal", bend: "Bend", mod_wheel: "Mod wheel", pressure: "Pressure" };
/** The MIDI controllers a lane may move: every one but the mod wheel (1) and the pedal (64), which have lanes of their own. */
export const CONTROLLERS = Array.from({ length: 120 }, (_, number) => number).filter((number) => number !== 1 && number !== 64);
/** What MIDI calls the controllers that have a name. */
const CONTROLLER_NAMES: Record<number, string> = {
  2: "Breath", 4: "Foot", 5: "Portamento time", 7: "Volume", 8: "Balance", 10: "Pan", 11: "Expression", 65: "Portamento", 66: "Sostenuto",
  67: "Soft pedal", 71: "Resonance", 72: "Release", 73: "Attack", 74: "Brightness", 91: "Reverb", 93: "Chorus",
};
/** A controller as its lane's picker lists it: its name with its number under it, or its number. */
export const controllerName = (number: number) => {
  const name = CONTROLLER_NAMES[number];
  return name ? { label: name, detail: `Controller ${number}` } : { label: `Controller ${number}` };
};

/** How far alt-up and alt-down move the chosen velocities, and Humanize a start and a velocity, either way. */
const VELOCITY_STEP = 10;
/** The shortest a note gets: 1/64. */
const SHORTEST = BEAT / 16;
const HUMANIZE = { timing: 20, velocity: 10 };

const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);
/** A note by what it is: an undo or an edit of another tool that moves notes in the list keeps it chosen. */
export const noteKey = (note: Frozen<Note>) => `${note.start}:${note.pitch}:${note.length}:${note.velocity}`;
/** Where the notes `chosen` names are in `notes`. */
const indexesOf = (notes: readonly Frozen<Note>[], chosen: readonly string[]) => new Set(notes.flatMap((note, index) => (chosen.includes(noteKey(note)) ? [index] : [])));

/**
 * `notes` with `chosen` moved by `ticks` and `pitches`, or copied there: the notes, and which are
 * chosen after. They stop together at tick 0, before `end` and at the lowest and highest pitch.
 */
export function moved(notes: readonly Note[], chosen: ReadonlySet<number>, ticks: number, pitches: number, copy: boolean, end: number) {
  const picked = [...chosen].map((index) => notes[index]!);
  const [starts, heights] = [picked.map((note) => note.start), picked.map((note) => note.pitch)];
  const by = clamp(ticks, -Math.min(...starts), end - 1 - Math.max(...starts));
  const up = clamp(pitches, -Math.min(...heights), 127 - Math.max(...heights));
  const shifted = (note: Note) => ({ ...note, start: note.start + by, pitch: note.pitch + up });
  if (!copy) return { notes: notes.map((note, index) => (chosen.has(index) ? shifted(note) : note)), chosen };
  const copies = [...chosen].map((index) => shifted(notes[index]!));
  return { notes: [...notes, ...copies], chosen: new Set(copies.map((_, index) => notes.length + index)) };
}

type Next = (notes: Note[], chosen: ReadonlySet<number>, end: number) => { notes: Note[]; chosen: ReadonlySet<number> };
/**
 * Changes the notes of the clip `key` as `next` says, given the places of the chosen ones and the
 * end of the clip, as one undo step, and chooses what it chose. A note added at or past the end is
 * left out: the timeline plays none there. A clip with no `length` plays to its last note.
 */
function change(song: Song, key: string, label: string, next: Next) {
  editClips(song, label, [key], (draft) => {
    const end = draft.length ?? Infinity;
    const result = next(draft.notes, indexesOf(draft.notes, song.memory.notes), end);
    draft.notes = result.notes.filter((note) => note.start < end);
    song.memory.notes = [...result.chosen].flatMap((index) => {
      const note = result.notes[index];
      return note && note.start < end ? [noteKey(note)] : [];
    });
  });
}

/** What the note menu and Q do: to the chosen notes, or to all while none is. */
export type Operation = "quantize" | "humanize" | "legato";
const OPERATIONS: Record<Operation, string> = { quantize: "Quantize notes", humanize: "Humanize notes", legato: "Make notes legato" };
/** How strong Quantize is, and how much it swings, as the note menu offers them. */
export const STRENGTHS = [1, 0.75, 0.5] as const;
export const SWINGS = [{ label: "No swing", swing: 0 }, { label: "Light", swing: 1 / 3 }, { label: "Medium", swing: 2 / 3 }, { label: "Full", swing: 1 }] as const;

/** Quantizes the notes to the song's grid of the snap (1/16 while it is off) at its strength and swing, humanizes or makes them legato, as one undo step. They stay chosen and inside the clip. */
export function operate(song: Song, key: string, operation: Operation) {
  const clip = clipAt(song, key);
  if (!clip) return;
  const { memory } = song;
  const seed = operation === "humanize" ? ++memory.humanized : 0;
  const last = (clip.length ?? Infinity) - 1;
  const runs = song.clock.tempo_map.time_signatures;
  /** `picked` quantized to the song's grid: each bar's notes from its own line, the clip's ticks. */
  const quantized = (picked: Note[]) => {
    const done = [...picked];
    const bars = Map.groupBy(picked.map((_, at) => at), (at) => barAt(runs, clip.start + picked[at]!.start).start);
    for (const [line, ats] of bars) {
      const moved = quantize(ats.map((at) => ({ ...picked[at]!, start: clip.start + picked[at]!.start - line })), { grid: stepAt(runs, line, memory.snap), strength: memory.strength, swing: memory.swing });
      ats.forEach((at, index) => (done[at] = { ...moved[index]!, start: moved[index]!.start + line - clip.start }));
    }
    return done;
  };
  change(song, key, OPERATIONS[operation], (notes, chosen) => {
    const places = chosen.size > 0 ? [...chosen] : notes.map((_, index) => index);
    const picked = places.map((index) => notes[index]!);
    const done = operation === "quantize" ? quantized(picked) : operation === "humanize" ? humanize(picked, { seed, ...HUMANIZE }) : legato(picked);
    const next = [...notes];
    places.forEach((index, at) => (next[index] = { ...done[at]!, start: clamp(done[at]!.start, 0, last) }));
    return { notes: next, chosen };
  });
}

/** From the earliest start of `notes` to their latest end, and their earliest start. */
function spanOf(notes: readonly Note[]) {
  const start = Math.min(...notes.map((note) => note.start));
  return { start, span: Math.max(...notes.map((note) => note.start + note.length)) - start };
}

/** What a key does to the notes of the open clip. */
export type NoteCommand = "all" | "add" | "paste" | "quantize" | "delete" | "copy" | "cut" | "duplicate" | "louder" | "softer" | "shorter" | "longer" | "none" | keyof typeof NOTE_MOVES;
/** How far each move takes the chosen notes: in steps of the grid, and in pitches. */
const NOTE_MOVES = { left: [-1, 0], right: [1, 0], up: [0, 1], down: [0, -1], "octave-up": [0, 12], "octave-down": [0, -12] } as const;

/** The clip the editor shows, `row/clip`, while it shows one. */
export const openClip = ({ memory, children }: Pick<Song, "memory" | "children">) => {
  const key = memory.below?.kind === "editor" ? memory.below.clip : null;
  return key && itemAt(children, key)?.tool === "timeline" ? key : null;
};
const clipAt = (song: Pick<Song, "children">, key: string) => {
  const clip = itemAt(song.children, key);
  return clip?.tool === "timeline" ? clip.state : undefined;
};
/** The song's grid in the ticks of a clip that starts at `start`: the cell `x` is in, and `x` moved `by` lines. */
const gridIn = ({ clock, memory }: Pick<Song, "clock" | "memory">, start: number) => {
  const runs = clock.tempo_map.time_signatures;
  return {
    cell: (x: number) => Math.max(cellOf(runs, start + x, memory.snap) - start, 0),
    step: (x: number, by: number) => stepped(runs, start + x, memory.snap, by) - start,
  };
};

/**
 * Does `command` to the notes of the open clip; whether there was something to do: all but
 * choosing them all, adding one and a paste need chosen notes. A paste goes to the playhead when
 * it is inside the clip, else right after the chosen notes, else to the start of the clip. A note
 * added goes right after the chosen ones, at the pitch of the last, so adding again makes a row
 * up to the end of the clip; with none chosen, on the grid where a paste goes, at C4.
 */
export function noteCommand(song: Song, command: NoteCommand): boolean {
  const key = openClip(song);
  const clip = key ? clipAt(song, key) : undefined;
  if (!key || !clip) return false;
  const { memory } = song;
  const playhead = song.clock.tick;
  // Notes an undo or another tool took away are no longer chosen.
  const chosen = indexesOf(clip.notes, memory.notes);
  const picked = () => [...chosen].map((index) => clip.notes[index]!);
  const without: Next = (notes, gone) => ({ notes: notes.filter((_, index) => !gone.has(index)), chosen: new Set<number>() });
  const inside = playhead >= clip.start && playhead < clip.start + lengthOf(clip);
  if (command === "all") {
    memory.notes = clip.notes.map(noteKey);
  } else if (command === "add") {
    const last = picked().reduce<Frozen<Note> | undefined>((latest, note) => (latest && latest.start + latest.length >= note.start + note.length ? latest : note), undefined);
    const start = last ? last.start + last.length : inside ? gridIn(song, clip.start).cell(playhead - clip.start) : 0;
    if (start >= lengthOf(clip)) return false;
    const note = { start, length: memory.length, pitch: last?.pitch ?? 60, velocity: 100 };
    change(song, key, "Add a note", (notes) => ({ notes: [...notes, note], chosen: new Set([notes.length]) }));
  } else if (command === "paste") {
    if (!memory.copied) return false;
    const at = inside ? playhead - clip.start : chosen.size > 0 ? (({ start, span }) => start + span)(spanOf(picked())) : 0;
    const pasted = memory.copied.map((note) => ({ ...note, start: note.start + at }));
    change(song, key, "Paste notes", (notes) => ({ notes: [...notes, ...pasted], chosen: new Set(pasted.map((_, index) => notes.length + index)) }));
  } else if (command === "quantize") {
    operate(song, key, "quantize");
  } else if (command === "none") {
    if (memory.notes.length === 0) return false;
    memory.notes = [];
  } else if (chosen.size === 0) {
    return false;
  } else if (command === "delete") {
    change(song, key, "Delete notes", without);
  } else if (command === "copy" || command === "cut") {
    const { start } = spanOf(picked());
    memory.copied = picked().map((note) => ({ ...note, start: note.start - start }));
    if (command === "cut") change(song, key, "Cut notes", without);
  } else if (command === "duplicate") {
    const { span } = spanOf(picked());
    change(song, key, "Duplicate notes", (notes, all) => moved(notes, all, span, 0, true, Infinity));
  } else if (command === "shorter" || command === "longer") {
    const { step } = gridIn(song, clip.start);
    const by = command === "longer" ? 1 : -1;
    change(song, key, "Resize notes", (notes, all) => ({ notes: notes.map((note, index) => (all.has(index) ? { ...note, length: Math.max(step(note.start + note.length, by) - note.start, SHORTEST) } : note)), chosen: all }));
  } else if (command === "louder" || command === "softer") {
    const by = command === "louder" ? VELOCITY_STEP : -VELOCITY_STEP;
    change(song, key, "Change velocity", (notes, all) => ({ notes: notes.map((note, index) => (all.has(index) ? { ...note, velocity: clamp(note.velocity + by, 1, 127) } : note)), chosen: all }));
  } else {
    const [steps, pitches] = NOTE_MOVES[command];
    const { start } = spanOf(picked());
    const ticks = steps === 0 ? 0 : gridIn(song, clip.start).step(start, steps) - start;
    change(song, key, "Move notes", (notes, all, end) => moved(notes, all, ticks, pitches, false, end));
  }
  return true;
}

const PITCH_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
/** A pitch as the editor says it, `C4` for 60. */
export const pitchName = (pitch: number) => `${PITCH_NAMES[pitch % 12]}${Math.floor(pitch / 12) - 1}`;
/** The pitch `text` names, with `#` or `b`: `C#4`, `Bb3`. `null` for none from 0 to 127. */
export function pitchOf(text: string): number | null {
  const [, letter, accidental, octave] = /^([a-g])([#b]?)\s*(-?\d+)$/i.exec(text.trim()) ?? [];
  if (!letter) return null;
  const pitch = PITCH_NAMES.indexOf(letter.toUpperCase()) + (accidental === "#" ? 1 : accidental === "b" ? -1 : 0) + (Number(octave) + 1) * 12;
  return pitch >= 0 && pitch <= 127 ? pitch : null;
}

/** A whole note in ticks, which a length is said in parts of. */
const WHOLE = 4 * BEAT;
/** The note a length is said in, as the 16 of 3/16: the snap's (a quarter for a bar, a sixteenth for none), or the first finer one it is whole in, down to 1/64. */
export const noteValueOf = (length: number, snap: number) => {
  const coarsest = WHOLE / Math.min(snap || BEAT / 4, BEAT);
  return [4, 8, 16, 32, 64].find((value) => value >= coarsest && length % (WHOLE / value) === 0) ?? 64;
};
/** A length as the editor says it: `3/16`. */
export const lengthText = (length: number, snap: number) => {
  const value = noteValueOf(length, snap);
  return `${Math.round((length * value) / WHOLE)}/${value}`;
};
/** The ticks `text` says a note is long: `3/16`, or `3` of the note `value` it is said in now. `null` below 1/64. */
export function lengthOfText(text: string, value: number): number | null {
  const [, count, under] = /^(\d+(?:\.\d+)?)(?:\s*\/\s*(\d+))?$/.exec(text.trim()) ?? [];
  const ticks = Math.round((Number(count) * WHOLE) / (under ? Number(under) : value));
  return Number.isFinite(ticks) && ticks >= SHORTEST ? ticks : null;
}

/** A point of an expression lane as the record holds it: the pedal counts from `start`, the others from `tick`. */
type Point = { at: number; value: number };
type PointLane = Exclude<NoteLane, "velocity">;
const limitsOf = (lane: PointLane): [number, number] => (lane === "bend" ? [-8192, 8191] : [0, 127]);
/** The points of an expression lane of `clip`, as `at` and `value`. */
export function pointsOf(clip: Frozen<Clip>, lane: PointLane): Point[] {
  const points = lane === "pedal" ? clip.pedal.map(({ start, value }) => ({ tick: start, value })) : typeof lane === "number" ? (clip.controllers[lane] ?? []) : clip[lane];
  return points.map(({ tick, value }) => ({ at: tick, value }));
}
/** Puts `points` into the lane, one a tick, in order, as the record needs them; a controller lane with none goes. */
function setPoints(clip: Clip, lane: PointLane, points: Point[]) {
  const sorted = [...new Map(points.map((point) => [point.at, point])).values()].sort((a, b) => a.at - b.at);
  const ticked = sorted.map(({ at, value }) => ({ tick: at, value: Math.round(value) }));
  if (lane === "pedal") clip.pedal = ticked.map(({ tick, value }) => ({ start: tick, value }));
  else if (typeof lane !== "number") clip[lane] = ticked;
  else if (ticked.length > 0) clip.controllers = { ...clip.controllers, [lane]: ticked };
  else clip.controllers = Object.fromEntries(Object.entries(clip.controllers).filter(([number]) => Number(number) !== lane));
}
/** The value `y` down an expression lane, 0 its top. */
const valueDown = (lane: PointLane, y: number) => {
  const [low, high] = limitsOf(lane);
  return low + clamp(1 - y, 0, 1) * (high - low);
};

/** What a view does to the notes of the open clip `key`: each change one undo step. `p` is in the plane of what it was on. */
export const noteEdits = (song: Song, key: string) => {
  const { memory } = song;
  const notes = () => clipAt(song, key)?.notes ?? [];
  const edit = (label: string, next: Next) => change(song, key, label, next);
  const all = (each: (note: Note) => Note): Next => (list, chosen) => ({ notes: list.map((note, index) => (chosen.has(index) ? each(note) : note)), chosen });
  /** The earliest chosen note, which the head says. */
  const said = () => notes().filter((note) => memory.notes.includes(noteKey(note))).reduce<Frozen<Note> | undefined>((first, note) => (!first || note.start < first.start ? note : first), undefined);
  const hear = (pitch: number) => song.play(pitch, { child: rowOfKey(key) as `audio-${string}`, seconds: 0.3 });
  const grid = () => gridIn(song, clipAt(song, key)?.start ?? 0);
  return {
    /** A press on a note: it sounds; shift or cmd adds it or takes it out; a right press chooses it for the menu; a plain press chooses it alone. */
    press(note: Frozen<Note>, p: Pick<Pointer, "shift" | "cmd" | "right">) {
      const chosen = noteKey(note);
      if (p.right) {
        if (!memory.notes.includes(chosen)) memory.notes = [chosen];
        return;
      }
      hear(note.pitch);
      if (p.shift || p.cmd) memory.notes = memory.notes.includes(chosen) ? memory.notes.filter((each) => each !== chosen) : [...memory.notes, chosen];
      else memory.notes = [chosen];
    },
    /** A drag that began on a note not chosen: it alone is chosen, and the drag moves it. */
    choose(note: Frozen<Note>) {
      memory.notes = [noteKey(note)];
    },
    /** A press where no note is: a double click adds one on the snap, as long as the last, and sounds it; a plain press lets go of the chosen. */
    pressEmpty(p: Pick<Pointer, "x" | "y" | "clicks" | "shift" | "cmd">) {
      if (p.clicks === 2) {
        const note = { start: grid().cell(p.x), length: memory.length, pitch: clamp(127 - Math.floor(p.y), 0, 127), velocity: 100 };
        hear(note.pitch);
        edit("Add a note", (list) => ({ notes: [...list, note], chosen: new Set([list.length]) }));
      } else if (!p.shift && !p.cmd) memory.notes = [];
    },
    /** A double click on a note: it goes. */
    remove(note: Frozen<Note>) {
      edit("Delete notes", (list) => ({ notes: list.filter((each) => noteKey(each) !== noteKey(note)), chosen: new Set() }));
    },
    /** A drag of the chosen notes by `dx` ticks and `dy` rows: alt copies them. One moved to another pitch sounds there. */
    move(p: Pick<Pointer, "dx" | "dy" | "alt">) {
      const pitches = -Math.round(p.dy);
      const first = said();
      if (first && pitches !== 0) hear(clamp(first.pitch + pitches, 0, 127));
      edit(p.alt ? "Copy notes" : "Move notes", (list, chosen, end) => moved(list, chosen, Math.round(p.dx), pitches, p.alt, end));
    },
    /** A drag of the start of the chosen notes: their ends stay, the earliest stops at 0, and each keeps at least 1/64. */
    resizeStart(p: Pick<Pointer, "dx">) {
      const by = Math.max(Math.round(p.dx), -(said()?.start ?? 0));
      edit("Resize notes", all((note) => {
        const start = Math.min(note.start + by, note.start + note.length - SHORTEST);
        return { ...note, start, length: note.start + note.length - start };
      }));
    },
    /** A drag of the end of the chosen notes: at least 1/64; a new note is as long as the last one resized. */
    resize(p: Pick<Pointer, "dx">) {
      edit("Resize notes", (list, chosen) => {
        const changed = list.map((note, index) => (chosen.has(index) ? { ...note, length: Math.max(note.length + Math.round(p.dx), SHORTEST) } : note));
        const last = [...chosen].at(-1);
        if (last !== undefined && changed[last]) memory.length = changed[last].length;
        return { notes: changed, chosen };
      });
    },
    /** A drag of a bar of the velocity lane by `dy`: every chosen note as much. */
    velocity(p: Pick<Pointer, "dy">) {
      edit("Change velocity", all((note) => ({ ...note, velocity: Math.round(clamp(note.velocity - p.dy, 1, 127)) })));
    },
    /** A line drawn across the velocity lane: each note it passes gets the velocity of its height there. */
    drawVelocities(p: Pick<Pointer, "x" | "y" | "dx" | "dy">) {
      const [left, right] = [Math.min(p.x, p.x + p.dx), Math.max(p.x, p.x + p.dx)];
      const heightAt = (tick: number) => (p.dx === 0 ? p.y + p.dy : p.y + ((tick - p.x) * p.dy) / p.dx);
      edit("Draw velocities", (list, chosen) => ({
        notes: list.map((note) => (note.start >= left && note.start <= right ? { ...note, velocity: Math.round(clamp(127 - heightAt(note.start), 1, 127)) } : note)),
        chosen,
      }));
    },
    /** A drag over empty roll: the notes it touches are chosen; shift or cmd adds them. */
    marquee(p: Pick<Pointer, "x" | "y" | "dx" | "dy" | "shift" | "cmd">) {
      const [left, right] = [Math.min(p.x, p.x + p.dx), Math.max(p.x, p.x + p.dx)];
      const [top, bottom] = [Math.min(p.y, p.y + p.dy), Math.max(p.y, p.y + p.dy)];
      const inside = notes().flatMap((note) => {
        const row = 127 - note.pitch;
        return note.start < right && note.start + note.length > left && row < bottom && row + 1 > top ? [noteKey(note)] : [];
      });
      memory.notes = [...new Set([...(p.shift || p.cmd ? memory.notes : []), ...inside])];
    },
    operate: (operation: Operation) => operate(song, key, operation),
    /** The values of the head, typed: a pitch by name, a start as bar.beat.sixteenth of the song, a length as 3/16, a velocity 1 to 127. Unreadable text changes nothing. */
    typePitch(text: string) {
      const [note, pitch] = [said(), pitchOf(text)];
      if (note && pitch !== null) edit("Move notes", (list, chosen, end) => moved(list, chosen, 0, pitch - note.pitch, false, end));
    },
    typeStart(tick: number | null) {
      const [note, clip] = [said(), clipAt(song, key)];
      if (!note || !clip || tick === null) return;
      const at = tick - clip.start;
      if (at >= 0 && at < lengthOf(clip)) edit("Move notes", (list, chosen, end) => moved(list, chosen, at - note.start, 0, false, end));
    },
    typeLength(text: string) {
      const note = said();
      const ticks = note ? lengthOfText(text, noteValueOf(note.length, memory.snap)) : null;
      if (ticks === null) return;
      memory.length = ticks;
      edit("Resize notes", all((each) => ({ ...each, length: ticks })));
    },
    typeVelocity(text: string) {
      const loud = /^\d+$/.test(text.trim()) ? clamp(Number(text), 1, 127) : null;
      if (loud !== null) edit("Change velocity", all((each) => ({ ...each, velocity: loud })));
    },
    /** A double click on an expression lane: a point on the snap. */
    addPoint(lane: PointLane, p: Pick<Pointer, "x" | "y">) {
      editClips(song, "Add a point", [key], (clip) => setPoints(clip, lane, [...pointsOf(clip, lane), { at: grid().cell(p.x), value: valueDown(lane, p.y) }]));
    },
    /** A drag of the point at `at` of an expression lane, on the snap. */
    movePoint(lane: PointLane, at: number, p: Pick<Pointer, "dx" | "dy">) {
      editClips(song, "Move a point", [key], (clip) => {
        const [low, high] = limitsOf(lane);
        setPoints(clip, lane, pointsOf(clip, lane).map((point) => (point.at === at ? { at: Math.max(at + Math.round(p.dx), 0), value: clamp(point.value - p.dy * (high - low), low, high) } : point)));
      });
    },
    /** An alt press on a point: it goes, and a controller lane with none goes too. */
    removePoint(lane: PointLane, at: number) {
      editClips(song, "Remove a point", [key], (clip) => setPoints(clip, lane, pointsOf(clip, lane).filter((point) => point.at !== at)));
    },
  };
};
