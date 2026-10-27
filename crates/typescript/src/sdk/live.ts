// Views: nodes the code makes once, which the window keeps. What moves on them (a drag, a
// playhead, a meter, a knob turned) moves in the window, without running code: a live value.

import { splitPath } from "./components";
import { IconName } from "./drawing";
import { InputNames, Inputs, Unit } from "./fields";
import { ChildTools, Context, Frozen } from "./tools";

/**
 * The clock of the project, one for all tools: where the piece plays and how fast. A binding
 * that reads `playing`, `repeat`, `click`, `tempo_map` or `sections` runs again when it changes.
 * `tick`, `bar`, `beat` and `bpm` move on every frame: read them in a handler, `tick` or
 * `onKey`, and show them with `now.tick` and `now.bpm`, which move without code.
 */
export interface Clock {
  readonly playing: boolean;
  /** The position in ticks, 960 to a quarter note, as the code runs. */
  readonly tick: number;
  /** The bar and the beat of `tick`, from 1. */
  readonly bar: number;
  readonly beat: number;
  /** The tempo at `tick`, in beats per minute. */
  readonly bpm: number;
  /** The ticks playback goes back from the end to the start of, while it repeats. */
  readonly repeat: { readonly from: number; readonly to: number } | null;
  /** Whether the click sounds the beats. Not saved. */
  readonly click: boolean;
  /** Whether the computer keys play notes, as a MIDI keyboard does. A switch of this machine, not saved; cmd-K turns it too. */
  readonly computerKeys: boolean;
  /** As `project.json` holds them: the time signatures and the tempo changes. */
  readonly tempo_map: Frozen<TempoMap>;
  /** As `project.json` holds them: the named parts of the piece, sorted by bar. */
  readonly sections: Frozen<Section[]>;
  play(): void;
  pause(): void;
  /** Pauses and goes back to the start. */
  stop(): void;
  /** Playback goes on from `to`: at once, or with `atTick` once it reaches that tick, on its frame. */
  jump(to: number, options?: { atTick?: number }): void;
  /** Playback goes back from `to` to `from` while it is set; `null` plays on. Not saved. */
  setRepeat(range: { from: number; to: number } | null): void;
  setClick(on: boolean): void;
  setComputerKeys(on: boolean): void;
  /**
   * Changes the tempo map and the sections: `change` gets a copy to edit, and what `project.json`
   * does not accept is refused whole. Updates of one `label` in a row are one undo step.
   */
  update(label: string, change: (song: { tempo_map: TempoMap; sections: Section[] }) => void): void;
}

/** The time signatures, as runs of bars, and the tempo from a tick on, as steps. */
export interface TempoMap {
  time_signatures: { signature: string; bars: number }[];
  tempo_changes: { tick: number; bpm: number }[];
}

/** A part of the piece from the start of `bar`, counted from 1, up to the next section. */
export interface Section {
  bar: number;
  name: string;
}

/** What a view gets: what all code gets, and the room the window gives it. */
export interface View<State, I extends Inputs = Inputs, M = unknown, K extends ChildTools = ChildTools>
  extends Context<State, I, M, K> {
  /**
   * Its room in points, which changes with the window: read it in a binding. A card in a rack is
   * as tall as its room and as wide as it needs; at the top of a project it is the whole window.
   */
  size: { width: number; height: number };
}

// Live values

/** What a live value is made of, as the window gets it. */
type LiveNode =
  | { op: "tick" | "elapsed" | "bpm" }
  | { op: "value"; name: string; child?: string; played: boolean; fraction: boolean }
  | { op: "peak"; name: string; channel?: Channel }
  | { op: "device_peak"; channel?: Channel }
  | { op: "input"; channel: number }
  | { op: "drag"; id: string; part: keyof DragValues }
  | { op: "space"; key: string; part: keyof SpaceValues }
  | { op: "hover"; key: string }
  | { op: "plus" | "minus" | "times" | "over" | "mod" | "pow" | "min" | "max" | "lt" | "gt" | "le" | "ge" | "eq" | "ne" | "snap"; a: Num; b: Num }
  | { op: "negate" | "abs" | "floor" | "round" | "sin" | "cos" | "decibels"; x: Num }
  | { op: "ease"; x: Num; ease: EaseNode; key: string };

type EaseNode = { kind: "smooth"; ms: number } | { kind: "fall"; per_second: number } | { kind: "hold"; seconds: number };

/** 0 the left channel, 1 the right. */
export type Channel = 0 | 1;

/** A short name of what a live value is made of: the same value drawn twice keeps one memory. */
function liveKey(value: unknown): string {
  const text = JSON.stringify(value);
  let hash = 2166136261;
  for (let index = 0; index < text.length; index++) {
    hash = Math.imul(hash ^ text.charCodeAt(index), 16777619);
  }
  return (hash >>> 0).toString(36) + text.length.toString(36);
}

/**
 * A number the window works out again on every frame, without running code: where the
 * playhead is, a field as a drag turns it, an output of the sound, how far a drag went. Combine them with the
 * methods, which take a live value or a number. Code cannot read one: it is no number, so
 * `if (now.tick > 960)` is a type error.
 */
export class Live {
  constructor(readonly node: LiveNode) {}
  toJSON(): LiveNode { return this.node; }
  plus(other: Num): Live { return new Live({ op: "plus", a: this, b: other }); }
  minus(other: Num): Live { return new Live({ op: "minus", a: this, b: other }); }
  times(other: Num): Live { return new Live({ op: "times", a: this, b: other }); }
  over(other: Num): Live { return new Live({ op: "over", a: this, b: other }); }
  min(other: Num): Live { return new Live({ op: "min", a: this, b: other }); }
  max(other: Num): Live { return new Live({ op: "max", a: this, b: other }); }
  clamp(low: Num, high: Num): Live { return this.max(low).min(high); }
  /** 1 where the comparison holds, else 0. */
  lt(other: Num): Live { return new Live({ op: "lt", a: this, b: other }); }
  gt(other: Num): Live { return new Live({ op: "gt", a: this, b: other }); }
  le(other: Num): Live { return new Live({ op: "le", a: this, b: other }); }
  ge(other: Num): Live { return new Live({ op: "ge", a: this, b: other }); }
  eq(other: Num): Live { return new Live({ op: "eq", a: this, b: other }); }
  ne(other: Num): Live { return new Live({ op: "ne", a: this, b: other }); }
  /** The nearest whole number of `step`s: `dx.snap(240)` moves by sixteenths. */
  snap(step: Num): Live { return new Live({ op: "snap", a: this, b: step }); }
  negate(): Live { return new Live({ op: "negate", x: this }); }
  abs(): Live { return new Live({ op: "abs", x: this }); }
  floor(): Live { return new Live({ op: "floor", x: this }); }
  round(): Live { return new Live({ op: "round", x: this }); }
  /** Of an angle in radians. */
  sin(): Live { return new Live({ op: "sin", x: this }); }
  cos(): Live { return new Live({ op: "cos", x: this }); }
  /** What is left over after whole `other`s, from 0 up: `now.elapsed.mod(1)` goes 0 to 1 every second. */
  mod(other: Num): Live { return new Live({ op: "mod", a: this, b: other }); }
  pow(exponent: Num): Live { return new Live({ op: "pow", a: this, b: exponent }); }
  /** An amplitude in decibels: 1 is 0 dB; -120 at the lowest. */
  decibels(): Live { return new Live({ op: "decibels", x: this }); }
  /** Glides to it, about two thirds of the way in `ms`. */
  smooth(ms: number): Live { return this.ease({ kind: "smooth", ms }); }
  /** Rises with it at once and falls at most `perSecond` a second: a meter's bar. */
  fall(perSecond: number): Live { return this.ease({ kind: "fall", per_second: perSecond }); }
  /** Rises with it at once and stays `seconds` before it follows it: a meter's peak line, `level.hold(1.5).fall(0.3)`. */
  hold(seconds: number): Live { return this.ease({ kind: "hold", seconds }); }
  private ease(ease: EaseNode): Live { return new Live({ op: "ease", x: this, ease, key: liveKey([this, ease]) }); }
}

/** A number, or a live value the window works out on every frame. */
export type Num = number | Live;

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

/** What a wave draws: see `now.scope`, `now.history` and `now.file`. */
export type WaveList =
  | { source: "scope"; name: string; channel?: Channel; frames?: number }
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
   * A number as it is now: a field of the record, also as a drag turns it; a number input,
   * where it was put last; or a number output of the sound. A child's, or one deeper, as
   * `"bass/synth.cutoff_hz"`: its field, or its output. With `played`, a field as it plays: what a
   * lane or a cable plays into it, else the record, so a knob moves with its automation. With
   * `fraction`, how far along its range it is, 0 to 1, on a log scale for `"hz"`.
   */
  value(name: ValueNames<State, I>, options?: { played?: boolean; fraction?: boolean }): Live;
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
  /** The last sound of a `meter` child by its path, for a `Wave`: -1 to 1, `frames` long (at most 8192). */
  scope(name: string, options?: { channel?: Channel; frames?: number }): WaveList;
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

function sources(): Now<Record<string, number>> {
  const part = <Part extends string>(make: (part: Part) => LiveNode) => (parts: readonly Part[]) =>
    Object.fromEntries(parts.map((each) => [each, new Live(make(each))])) as Record<Part, Live>;
  return {
    tick: new Live({ op: "tick" }),
    bpm: new Live({ op: "bpm" }),
    elapsed: new Live({ op: "elapsed" }),
    value: (path, options) => {
      const { child, name } = splitPath(path);
      return new Live({ op: "value", name, ...(child === undefined ? {} : { child }), played: options?.played ?? false, fraction: options?.fraction ?? false });
    },
    peak: (name, channel) => new Live({ op: "peak", name, channel }),
    devicePeak: (channel) => new Live({ op: "device_peak", channel }),
    input: (channel) => new Live({ op: "input", channel }),
    scope: (name, options) => ({ source: "scope", name, ...options }),
    history: (of, frames) => ({ source: "history", of, frames, key: liveKey([of, frames]) }),
    file: (asset, options) => ({ source: "file", asset, ...options }),
    take: (name) => ({ source: "take", name }),
    drag: (id) =>
      part<keyof DragValues>((each) => ({ op: "drag", id, part: each }))(["startX", "startY", "dx", "dy", "x", "y", "active"]),
    space: (key) =>
      part<keyof SpaceValues>((each) => ({ op: "space", key, part: each }))(["left", "top", "right", "bottom", "pointX", "pointY"]),
    hover: (key) => new Live({ op: "hover", key }),
  };
}

/** The sources of live values, for components of a view, which name fields by strings. */
export const now: Now<Record<string, number>> = sources();


/** The keys of the computer keyboard, as the window names them. */
export const PLAIN_KEYS = [
  "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
  "t", "u", "v", "w", "x", "y", "z", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "-", "=",
  "[", "]", ";", "'", ",", ".", "/", "`", "\\", "space", "enter", "escape", "backspace", "delete",
  "tab", "up", "down", "left", "right", "home", "end", "pageup", "pagedown", "f1", "f2", "f3",
  "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12"
] as const;
/** A key of the computer keyboard by its name. */
export type PlainKey = (typeof PLAIN_KEYS)[number];
/** A key by name, with cmd, ctrl and alt held first and in that order: `"space"`, `"cmd-c"`, `"cmd-alt-left"`. */
export type KeyName = `${"" | "cmd-"}${"" | "ctrl-"}${"" | "alt-"}${PlainKey}`;
/** The keys the app keeps: undo, new, open, quit, the panel, the computer keys, and tab. */
export type AppKey = `${"cmd" | "ctrl"}-${"z" | "n" | "o" | "q" | "l" | "b" | "k"}` | "tab";

/** An item of `openMenu`. */
export interface MenuItem {
  label: string;
  disabled?: boolean;
  /** A tick beside it. */
  checked?: boolean;
  icon?: IconName;
  /** A quiet second line, such as who made a plugin. */
  detail?: string;
  /** The heading it is under, with the items next to it of the same group. */
  group?: string;
  /** A line above it, between it and the items before. */
  separator?: boolean;
  /** The keys that do it too, drawn at the end of its row. */
  shortcut?: KeyName;
}

/** A device `offers` gives. */
export interface Offer {
  /** What `addOffer` names it by. */
  key: string;
  name: string;
  detail: string | null;
  /** The group a picker lists it under, such as "Built-in" or "Plugins". */
  group: string;
  /** The icon a picker shows beside it. */
  icon: IconName;
  /** Why it cannot be added here, or `null` when it can. */
  unavailable: string | null;
}

/** What `offers` gives: the devices, and what the window's pickers say under them, such as whose trademark a plugin format is. */
export interface Offers {
  offers: Offer[];
  notes: string[];
  /** The key of the offer at the child `offers` was asked `at`, which a picker ticks. */
  here: string | null;
}

/** A number `numbers` gives: what a lane or a cable moves, from `min` to `max`, as its knob travels them. */
/** How a menu of `openMenu` shows. */
export interface MenuOptions {
  /** Quiet lines under the items. */
  notes?: readonly string[];
  /** In points; 320 when left out. */
  width?: number;
  /** Past this many points the items scroll; 420 when left out. */
  maxHeight?: number;
  /** A field above the items that keeps those whose label holds what is typed. */
  search?: boolean;
}

export interface MovedNumber {
  name: string;
  /** What a person reads for it: as its device names it, such as a plugin's parameter, or its name in plain words, `Osc 1 position`. */
  label: string;
  min: number;
  max: number;
  /** `"log"` for a frequency or a time, `"fader"` for a volume in dB (`-inf` at the bottom). */
  scale: "linear" | "log" | "fader";
  /** How a value of it reads: by the unit its name ends in, `pan` for a pan, `percent` for a share of 0 to 1; none for a plugin's. */
  unit?: "hz" | "db" | "ms" | "seconds" | "percent" | "pan";
  /** What its record holds now; `null` for a number that has none, such as one of a plugin that is not loaded. */
  value: number | null;
}

// Pointer

/** Which modifier keys were held. */
export interface Modifiers {
  shift: boolean;
  alt: boolean;
  cmd: boolean;
  ctrl: boolean;
}

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
 * `assets/audio/` under the names in `files`, or what a dragged `div` carries. `x` and `y` are
 * where, in the units of a space (0 on a `div`).
 */
export interface Dropped {
  files: string[];
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
 * A number a drag turns as it goes, in the window, with no code: a knob or a fader. `value`
 * names it as `now.value` does: a field of the record, or of a child's as
 * `"bass/synth.gain_db"`, which the whole drag changes as one undo step; or a number input,
 * which is played, not saved. Without `min` and `max` it takes the field's or the input's.
 */
export type DragTurns = { value: string } & {
  min?: number;
  max?: number;
  /** `"hz"` turns on a log scale. */
  unit?: Unit;
  /** Points of pointer travel for the whole range, up or right; 200 when left out. With shift, ten times as far. */
  travel?: number;
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
