// Views: nodes the code makes once, which the window keeps. What moves on them (a drag, a
// playhead, a meter, a knob turned) moves in the window, without running code: a live value.

import { InputNames, Inputs, Unit } from "./fields";
import { ChildTools, Context, Frozen, StateOfTool } from "./tools";
import type { Modifiers } from "./clock";
import { Live, liveKey, Num, splitPath } from "./values";
import type { Channel, LiveNode } from "./values";

export * from "./clock";
export * from "./values";

/** What a view gets: what all code gets, and the room the window gives it. */
export interface View<State, I extends Inputs = Inputs, M = unknown, K extends ChildTools = ChildTools>
  extends Context<State, I, M, K> {
  /**
   * Its room in points, which changes with the window: read it in a binding. A card in a rack is
   * as tall as its room and as wide as it needs; at the top of a project it is the whole window.
   */
  size: { width: number; height: number };
}

/** What a drag gives while the pointer holds it, in the units of its space. All 0 when no drag of its `id` is held. */
export interface DragValues {
  /** Where it was pressed. */
  startX: Live;
  startY: Live;
  /** How far it moved, rounded to its `snap`. */
  dx: Live;
  dy: Live;
  /** Where it is: the start and how far it moved. */
  x: Live;
  y: Live;
  /** 1 while the pointer holds it, and until its code answered the end. */
  active: Live;
  /** 1 while it is held with alt. */
  alt: Live;
}

/** What the window shows of a space, in its units. */
export interface SpaceValues {
  /** The edges of what shows. */
  left: Live;
  top: Live;
  right: Live;
  bottom: Live;
  /** How many units one point is: `width: space.pointX.times(6)` is 6 points wide at any zoom. */
  pointX: Live;
  pointY: Live;
}

/** The numbers of fields `State` holds. */
type NumberNames<State> = { [Name in keyof State]: State[Name] extends number ? Name : never }[keyof State] & string;

/** Which channel of a sound a scope shows, their mean when left out, and how many samples. */
type ScopeOptions = { channel?: Channel; frames?: number };

/** What a wave draws: see `now.scope`, `now.history` and `now.file`. */
export type WaveList =
  | ({ source: "scope"; name: string } & ScopeOptions)
  | { source: "history"; of: Num; frames: number; key: string }
  | { source: "file"; asset: string; from?: number | null; to?: number | null }
  | TakeList;

/** What a recorder records so far, see `now.take`. */
export type TakeList = { source: "take"; name: string };

/** The names a tool reads with `now.value`: its numbers, its number inputs, and any output, by any name. */
type ValueNames<State, I extends Inputs> = NumberNames<State> | InputNames<I, "number"> | (string & {});

/** The sources of live values, which all code gets as `now`. */
export interface Now<State = Record<string, unknown>, I extends Inputs = Inputs> {
  /** Where the piece plays, in ticks: it moves on every frame as it plays. */
  tick: Live;
  /** The tempo at `tick`, in beats per minute. */
  bpm: Live;
  /** Seconds since the view opened, on every frame: `now.elapsed.mod(1).lt(0.5)` blinks once a second. */
  elapsed: Live;
  /**
   * A number as it is now: a field of the record as it plays, which is what a lane or a cable
   * plays into it, else the record, also as a drag turns it; a number input, where it was put
   * last; or a number output of the sound. A child's, or one deeper, as
   * `"bass/synth.cutoff_hz"`: its field, or its output. A number inside a field as its record
   * holds it: `"kit.pads.36.volume_db"`, `"eq.bands[0].gain_db"`, or `"voices[2].detune"` of an
   * item of its own list. With `fraction`, how far along its range it is, 0 to 1, on a log
   * scale for `"hz"`.
   */
  value(name: ValueNames<State, I>, options?: { fraction?: boolean }): Live;
  /**
   * The loudest sample since the last frame, 0 to 1 and above when it clips: of a `meter` child
   * by its path, `"bass-meter"` or `"bass/meter"`, or of peaks a child keeps, as `"child.name"`.
   * The louder channel unless `channel` says.
   */
  peak(name: string, channel?: Channel): Live;
  /** The same, of what the device plays. */
  devicePeak(channel?: Channel): Live;
  /**
   * The loudest sample of channel `channel` of the audio input, from 0, at the window's last poll:
   * 0 while the input is closed. It is open while a `recorder` is armed or records.
   */
  input(channel: number): Live;
  /**
   * The last sound the tool itself plays, or with a path what its child there plays, such as
   * `"bass"` or `"bass/synth"` (what a `meter` child hears), for a `Wave`: the last `frames`
   * samples (2048 when left out, at most 8192), from where the sound rises through zero, so a
   * steady tone stands still. It is as tall as its wave, so a quiet sound shows as a loud one
   * does, and one that dies away sinks to a flat line: how loud it is is a `Meter`'s to show.
   * A child's is its sound before its parent changes it:
   * a child its parent silences still draws a wave, so gate the sound in the child, where its
   * picture should be flat while nothing is heard.
   */
  scope(options?: ScopeOptions): WaveList;
  scope(child: string, options?: ScopeOptions): WaveList;
  /** The last `frames` values of a live value, one a frame, for a `Wave`. */
  history(of: Num, frames: number): WaveList;
  /**
   * The waveform of a file under `assets/audio/`, such as `"take-1.wav"`, from `from` to `to`
   * seconds of it (all of it when left out), for a `Wave` with `band` from `low: -1` to `high: 1`.
   * Made in the background: it shows once it is ready.
   */
  file(asset: string, options?: { from?: number | null; to?: number | null }): WaveList;
  /** What a `recorder` child by its path records, as it grows: its sound for a `Wave`, in the form of `file`, and its notes for `Notes`. */
  take(name: string): TakeList;
  /** The drag of `id`, see `Drag`. */
  drag(id: string): DragValues;
  /** The space of `key`, see `Space`. */
  space(key: string): SpaceValues;
  /** 1 while the pointer is over the shape of `key`, and while it drags it: the readout of a point shows. */
  hover(key: string): Live;
}

/** 1 while the pointer is over a control, the keys are on it or a drag holds it: the control of the key `name`, or any that turns the field `name`. */
export const touched = (name: string): Live => new Live({ op: "touched", name });

/** 1 while a lane or a cable moves the field at `path` and holds its control, which then shows a mark. */
export const heldByLane = (path: string): Live => new Live({ op: "held", ...splitPath(path) });

function sources(): Now<Record<string, number>> {
  const part = <Part extends string>(make: (part: Part) => LiveNode) => (parts: readonly Part[]) =>
    Object.fromEntries(parts.map((each) => [each, new Live(make(each))])) as Record<Part, Live>;
  return {
    tick: new Live({ op: "tick" }),
    bpm: new Live({ op: "bpm" }),
    elapsed: new Live({ op: "elapsed" }),
    value: (path, options) => new Live({ op: "value", ...splitPath(path), fraction: options?.fraction ?? false }),
    peak: (name, channel) => new Live({ op: "peak", name, channel }),
    devicePeak: (channel) => new Live({ op: "device_peak", channel }),
    input: (channel) => new Live({ op: "input", channel }),
    scope: (child?: string | ScopeOptions, options?: ScopeOptions) =>
      typeof child === "string" ? { source: "scope", name: child, ...options } : { source: "scope", name: "", ...child },
    history: (of, frames) => ({ source: "history", of, frames, key: liveKey([of, frames]) }),
    file: (asset, options) => ({ source: "file", asset, ...options }),
    take: (name) => ({ source: "take", name }),
    drag: (id) =>
      part<keyof DragValues>((each) => ({ op: "drag", id, part: each }))(["startX", "startY", "dx", "dy", "x", "y", "active", "alt"]),
    space: (key) =>
      part<keyof SpaceValues>((each) => ({ op: "space", key, part: each }))(["left", "top", "right", "bottom", "pointX", "pointY"]),
    hover: (key) => new Live({ op: "hover", key }),
  };
}

/** The sources of live values, for components of a view, which name fields by strings. */
export const now: Now<Record<string, number>> = sources();

// Pointer

/** A press of the pointer: where in the units of its space (0 on a `div`), which button, and how many clicks in a row. */
export interface Press extends Modifiers {
  x: number;
  y: number;
  button: "left" | "right";
  clicks: number;
}

/** What a node hears of the pointer. Each hears the `Press` it follows. */
export interface PointerEvents {
  /** A button goes down on it. */
  onPress?: (press: Press) => void;
  /** That button goes up, wherever the pointer is by then, also after Escape, when the window loses focus or when the node is gone. */
  onRelease?: (press: Press) => void;
  /** A left press and release on it with no drag between. */
  onClick?: (press: Press) => void;
}

/**
 * What was let go on a node that hears drops: audio files from the system, copied into
 * `assets/audio/` under the names in `files`; MIDI files, one timeline at tick 0 for each part
 * (a channel of a track that plays notes) in `timelines`, its `title` the part's name; or what
 * a dragged `div` carries. `x` and `y` are where, in the units of a space (0 on a `div`).
 */
export interface Dropped {
  files: string[];
  timelines: StateOfTool<"timeline">[];
  carried: string | null;
  x: number;
  y: number;
}

/** A drag at its end: as `DragValues` says, in numbers. */
export interface DragResult extends Modifiers {
  startX: number;
  startY: number;
  dx: number;
  dy: number;
  x: number;
  y: number;
}

/**
 * How a drag or a key moved what code keeps, as the window measured it: `moved` along its travel
 * as a part of the whole, or in whole steps of its `step`, since the press of the drag while
 * `held` and else from where it is now; `at` a place along its travel, of an `absolute` drag;
 * or `reset`, to its default.
 */
export type Turn = { moved: number; held: boolean } | { at: number } | { reset: true };

/**
 * A number a drag turns as it goes, in the window: a knob or a fader. `value` names it as
 * `now.value` does: a field of the record, or of a child's as `"bass/synth.gain_db"`, which the
 * whole drag changes as one undo step; or a number input, which is played, not saved. Without
 * `min` and `max` it takes the field's or the input's. Or what code keeps, with `onTurn`: code
 * alone can read it, so the window tells each `Turn` of the drag and of the keys and the code
 * works out the value, as `Knob` does. Either way shift is ten times finer, a double click or
 * backspace is for `default`, and escape puts it back.
 */
export type DragTurns = ({ value: string; onTurn?: never } | { onTurn: (turn: Turn) => void; value?: never }) & {
  min?: number;
  max?: number;
  /** `"hz"` turns on a log scale. */
  unit?: Unit;
  /** The scale of a volume fader, in dB: 0 dB at 0.8 of the travel, -inf at its foot. */
  fader?: boolean;
  /** Points of pointer travel for the whole range, up or right; 200 when left out. With shift, ten times as far. */
  travel?: number;
  /** It turns in whole steps of this size from where it is, `per` points of travel each (10 when left out), and the arrows step it by one: a number box. `min` and `max` then only bound it. */
  step?: number;
  per?: number;
  /** The value is where the pointer is across the space (or up it), from the press on, as on a pad. */
  absolute?: boolean;
  /** A double click puts it here. */
  default?: number;
  /** The undo step of a field. "Change <field>" when left out. */
  label?: string;
};

/**
 * The pointer pressing and dragging a shape or a space. The window moves what binds to
 * `now.drag(id)` as the pointer goes, and turns `x` and `y`; code hears it once, at `onEnd`.
 * Until the changes its code makes then arrive, the drag's values stay, so what moved does not
 * jump back while its code runs. Make one and give it to every shape it drags.
 */
export interface Drag {
  /** Names its live values: `now.drag(id)`. */
  id?: string;
  /**
   * Rounds `dx` and `dy` to whole steps, in units. With `grid`, the pressed shape's `anchor` lands
   * on a whole step instead, as a note lands on the lines of a piano roll wherever it started.
   * While cmd is held `dx` does not snap, off the grid in time: round it where it must be whole.
   */
  snap?: { x?: number; y?: number; grid?: boolean };
  /** Moves along one axis only. */
  axis?: "x" | "y";
  /** What the drag turns across and up. */
  x?: DragTurns;
  y?: DragTurns;
  /** At the end of the drag, when the pointer moved. */
  onEnd?: (drag: DragResult) => void;
}
