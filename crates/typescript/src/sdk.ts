// The SDK of the project's own tools. Sound Tools writes this file when the project opens, so
// an edit here is lost. Read `agent-docs/extensions.md` first.
//
// A tool is a `.ts` or `.tsx` file next to this one that calls `tool`: the fields of its
// record, the controls its card plays, its doc for agents, its sound and its card.

// ---------------------------------------------------------------------------------------------
// Fields of a record: saved, undoable, and what an agent writes.

/** What a readout shows after the number. */
export type Unit = "hz" | "ms" | "db" | "percent";

/** A number a knob turns. It plays live: a turn glides the sound and runs no code. */
export interface KnobField {
  kind: "knob";
  min: number;
  max: number;
  default: number;
  unit?: Unit;
  /** What the card calls it. The field name, in words, when left out. */
  label?: string;
}

/** On or off. It plays live, as a signal of 1 or 0. */
export interface ToggleField {
  kind: "toggle";
  default: boolean;
  label?: string;
}

/**
 * One of a few options. It decides the structure of the sound, such as how many delays there
 * are, so `sound` runs for each option when the tool loads, and the sound of a new one fades in.
 */
export interface ChoiceField<Option extends string | number = string | number> {
  kind: "choice";
  options: readonly [Option, ...Option[]];
  default: Option;
  label?: string;
}

/** A list of numbers, such as the steps of a sequence. It plays live, as a `Table`. */
export interface PatternField {
  kind: "pattern";
  length: number;
  min: number;
  max: number;
  default: number;
  label?: string;
}

/**
 * A sound to play from, such as a recording: the record holds the name of a file under
 * `assets/audio/`, or "" for none. A `Table` in `sound`, at the rate of the engine, mixed to
 * one channel, up to 60 seconds.
 */
export interface SampleField {
  kind: "sample";
  label?: string;
}

// Data a behaviour, a card and a page read: no signal, so `sound` does not get them.

/** Notes in ticks, 960 to a quarter note, from where the parent's code plays them. */
export interface NotesField {
  kind: "notes";
  /** `[]` when left out. */
  default?: Note[];
  label?: string;
}

export interface TextField {
  kind: "text";
  /** `""` when left out. */
  default?: string;
  label?: string;
}

export interface NumberField {
  kind: "number";
  min: number;
  max: number;
  /** Only whole numbers. */
  integer?: boolean;
  /** `min` when left out. */
  default?: number;
  label?: string;
}

/** The fields of each item of a list. */
export interface ObjectItem<F extends Fields = Fields> {
  kind: "object";
  fields: F;
}

/** Items of one kind: a field, or an object of fields. `[]` when left out. */
export interface ListField<I extends Field | ObjectItem = Field | ObjectItem> {
  kind: "list";
  item: I;
  /** The most items it holds. */
  max?: number;
  /** `[]` when left out. `list` types it from `item`. */
  default?: unknown[];
  label?: string;
}

export type DataField = NotesField | TextField | NumberField | ListField;
export type Field = KnobField | ToggleField | ChoiceField | PatternField | SampleField | DataField;
export type Fields = Record<string, Field>;

/** A note of a `notes` field. */
export interface Note {
  /** In ticks from where it is played from. */
  start: number;
  /** In ticks, 1 or more. */
  length: number;
  /** MIDI, 0 to 127: 60 is middle C. */
  pitch: number;
  /** 1 to 127. */
  velocity: number;
}

export const knob = (field: Omit<KnobField, "kind">): KnobField => ({ kind: "knob", ...field });
export const toggle = (field: Omit<ToggleField, "kind">): ToggleField => ({ kind: "toggle", ...field });
/** `default` is one of `options`, which are at least one. */
export const choice = <const Option extends string | number>(field: {
  options: readonly [Option, ...Option[]];
  default: NoInfer<Option>;
  label?: string;
}): ChoiceField<Option> => ({ kind: "choice", ...field });
export const pattern = (field: Omit<PatternField, "kind">): PatternField => ({ kind: "pattern", ...field });
export const sample = (field: Omit<SampleField, "kind"> = {}): SampleField => ({ kind: "sample", ...field });
export const notes = (field: Omit<NotesField, "kind"> = {}): NotesField => ({ kind: "notes", ...field });
export const text = (field: Omit<TextField, "kind"> = {}): TextField => ({ kind: "text", ...field });
export const number = (field: Omit<NumberField, "kind">): NumberField => ({ kind: "number", ...field });

/**
 * The item of a list: a field as it is, an object of fields as an `ObjectItem`. Not `I & Fields`,
 * whose index signature would let a misspelled field of an item through.
 */
type ItemOf<I extends Field | Fields> = I extends Field ? I : I extends Fields ? ObjectItem<I> : never;

/**
 * A list whose items are `item`: a field, such as `notes()`, or an object of fields. A field an
 * item of `default` leaves out is at its own default.
 */
export function list<const I extends Field | Fields>(
  item: I,
  options: { max?: number; label?: string; default?: ValueOf<ListField<ItemOf<I>>, false> } = {},
): ListField<ItemOf<I>> {
  const field = typeof item.kind === "string" ? item : { kind: "object", fields: item };
  return { kind: "list", item: field as ItemOf<I>, ...options };
}

// ---------------------------------------------------------------------------------------------
// Controls: what the card plays and nothing saves, as a performer plays an instrument.

/** A number the card sets as it is played, such as an XY pad. Not saved; starts at `default`. */
export interface LiveControl {
  kind: "live";
  min: number;
  max: number;
  default: number;
  label?: string;
}

/** A bang: a signal that is 1 for the one sample after the card fires it. */
export interface TriggerControl {
  kind: "trigger";
  label?: string;
}

export type Control = LiveControl | TriggerControl;
export type Controls = Record<string, Control>;

/** The names of the controls in `C` of kind `K`, so `set` takes no trigger and `fire` no live control. */
type ControlNames<C extends Controls, K extends Control["kind"]> = {
  [Name in keyof C]: C[Name] extends infer Each ? (Each extends { kind: K } ? Name : never) : never;
}[keyof C] &
  string;

export const live = (control: Omit<LiveControl, "kind">): LiveControl => ({ kind: "live", ...control });
export const trigger = (control: Omit<TriggerControl, "kind"> = {}): TriggerControl => ({
  kind: "trigger",
  ...control,
});

/** A value of a field as a record holds it; `Filled` says whether the fields of an item may be left out. */
type ValueOf<F, Filled extends boolean> = F extends KnobField | NumberField
  ? number
  : F extends ToggleField
    ? boolean
    : F extends ChoiceField<infer Option>
      ? Option
      : F extends PatternField
        ? number[]
        : F extends SampleField | TextField
          ? string
          : F extends NotesField
            ? Note[]
            : F extends ListField<infer I>
              ? Array<I extends ObjectItem<infer G> ? (Filled extends true ? ValuesOf<G> : StateOf<G>) : ValueOf<I, Filled>>
              : never;

/** The `state` of a record as a file may hold it: a field left out is at its default. */
export type StateOf<S extends Fields> = { -readonly [Name in keyof S]?: ValueOf<S[Name], false> };

/**
 * The `state` of a record as a behaviour, a card, a page and the control loop get it: every
 * field, at its default where the record leaves it out. Not readonly as the fields of a tool
 * are, so `update` can change it.
 */
export type ValuesOf<S extends Fields> = { -readonly [Name in keyof S]: ValueOf<S[Name], true> };

/**
 * What `sound` gets: a choice as its value, a pattern as a `Table`, a data field not at all,
 * anything else as a `Param`, which is a `Signal`.
 */
export type SoundOf<S extends Fields, C extends Controls> = {
  [Name in keyof S as S[Name] extends DataField ? never : Name]: S[Name] extends ChoiceField<infer Option>
    ? Option
    : S[Name] extends PatternField | SampleField
      ? Table
      : Param;
} & { [Name in keyof C]: Param };

// ---------------------------------------------------------------------------------------------
// The sound graph: a sound is a `Signal`, built from the functions below. The SDK sends the graph
// to the runtime, which plays it sample by sample.

/** A signal, or a plain number. */
export type Operand = Signal | number;

/** What a sound reads from outside: what comes in, the transport, the note of the voice. */
type Source =
  | "input" | "inputLeft" | "inputRight" | "channel" | "sampleRate" | "beat" | "bpm" | "playing"
  | "frequency" | "pitch" | "gate" | "velocity" | "onset";
type Unary =
  | "negate" | "sin" | "cos" | "tan" | "tanh" | "abs" | "sqrt" | "exp" | "log" | "floor" | "wrap"
  | "db" | "saturate";
type Binary =
  | "add" | "subtract" | "multiply" | "divide" | "remainder" | "less" | "greater" | "lessOrEqual"
  | "greaterOrEqual" | "equal" | "notEqual" | "min" | "max" | "pow";

/** Where a table reads: a pattern or sample field by name, or a `buffer` by its number. */
export type TableRef = { list: string } | { buffer: number };

/** A node of the sound graph, with `R` for each signal it reads. */
type SignalNode<R> =
  | { op: Source }
  /** A field or a control. */
  | { op: "param"; name: string }
  | { op: "feedback"; slot: number }
  | { op: Unary; x: R }
  | { op: Binary; a: R; b: R }
  | { op: "clamp"; x: R; low: R; high: R }
  | { op: "mix"; a: R; b: R; amount: R }
  | { op: "phasor"; hz: R }
  | { op: "noise" }
  | { op: "delay"; x: R; ms: R; longest?: R }
  | { op: "lowpass" | "highpass" | "bandpass"; x: R; hz: R; q: R }
  | { op: "smooth"; x: R; ms: R }
  | { op: "adsr"; gate: R; attack: R; decay: R; sustain: R; release: R }
  | { op: "hold"; x: R; when: R }
  | { op: "rise" | "change"; x: R }
  | { op: "at"; table: TableRef; index: R }
  | { op: "lookup"; table: TableRef; phase: R }
  | { op: "length"; table: TableRef }
  /** What a device plays: of the channel the sound runs on, or of `channel`. */
  | { op: "played"; device: [R] extends [number] ? number : DeviceCall; channel?: 0 | 1 };

/** A node the runtime gets: each signal it reads is the index of a node before it. */
export type GraphNode =
  | SignalNode<number>
  | { op: "constant"; value: number }
  | { op: "write"; buffer: number; index: number; value: number }
  | { op: "device"; tool: string; input: [number, number]; numbers: Record<string, number>; state: Record<string, unknown> };

/** A sound as the runtime gets it. */
export interface Graph {
  nodes: GraphNode[];
  /** What each `feedback` is set to, by its slot. */
  feedbacks: number[];
  /** The seconds of each `buffer`. */
  buffers: number[];
  watches: Array<{ name: string; node: number }>;
  numbers: Array<{ name: string; node: number }>;
  bangs: Array<{ name: string; node: number }>;
  notes: Array<{ name: string; gate: number; pitch: number; velocity: number }>;
  output: { mono: number } | { left: number; right: number };
}

/**
 * A stream of samples: one value per sample, per channel, per voice. Combine signals with the
 * methods here and the functions below. A signal used in two places is one: one oscillator
 * heard twice, with one memory.
 */
export class Signal {
  constructor(readonly node: SignalNode<Operand>) {}
  plus(other: Operand): Signal { return new Signal({ op: "add", a: this, b: other }); }
  minus(other: Operand): Signal { return new Signal({ op: "subtract", a: this, b: other }); }
  times(other: Operand): Signal { return new Signal({ op: "multiply", a: this, b: other }); }
  over(other: Operand): Signal { return new Signal({ op: "divide", a: this, b: other }); }
  /** The remainder after division, always 0 or above for a divisor above 0. */
  mod(other: Operand): Signal { return new Signal({ op: "remainder", a: this, b: other }); }
  negate(): Signal { return new Signal({ op: "negate", x: this }); }
  /** 1 where the comparison holds, else 0. */
  lt(other: Operand): Signal { return new Signal({ op: "less", a: this, b: other }); }
  gt(other: Operand): Signal { return new Signal({ op: "greater", a: this, b: other }); }
  le(other: Operand): Signal { return new Signal({ op: "lessOrEqual", a: this, b: other }); }
  ge(other: Operand): Signal { return new Signal({ op: "greaterOrEqual", a: this, b: other }); }
  eq(other: Operand): Signal { return new Signal({ op: "equal", a: this, b: other }); }
  ne(other: Operand): Signal { return new Signal({ op: "notEqual", a: this, b: other }); }
}

/** A value read where it is used, with no memory: a source, a field, a control, a feedback. */
class Named extends Signal {
  declare readonly node: Extract<SignalNode<Operand>, { op: Source | "param" | "feedback" }>;
}

/**
 * A field or a control of the tool. A `Signal`, so the sound uses it as any other; it is no
 * number, so code cannot branch on it: a turn of a knob must not run `sound` again.
 */
export class Param extends Named {
  constructor(readonly name: string) {
    super({ op: "param", name });
  }
}

/** A list to read by index or by phase: a pattern of the record, or a `buffer`. */
export class Table {
  constructor(readonly ref: TableRef) {}
  /** The value at `index`: its whole part, wrapped, so any index reads. */
  at(index: Operand): Signal {
    return new Signal({ op: "at", table: this.ref, index });
  }
  /** How many values it holds. */
  get length(): Signal {
    return new Signal({ op: "length", table: this.ref });
  }
}

/** Memory to write and read, such as a loop or a grain cloud: all 0 at first. */
export class Buffer extends Table {
  constructor(private readonly slot: number) {
    super({ buffer: slot });
  }
  /** Writes `value` at `index`, every sample. */
  write(index: Operand, value: Operand): void {
    building().writes.push({ buffer: this.slot, index, value });
  }
}

/** A value that feeds back: reading it gives what it was `set` to one sample before. */
export class Feedback extends Named {
  value: Operand | undefined;
  constructor(slot: number) {
    super({ op: "feedback", slot });
  }
  /** What it is in the next sample. Set it once. */
  set(value: Operand): void {
    if (this.value !== undefined) throw new Error("a feedback is set once");
    this.value = value;
  }
}

/** What one call of `sound` declares besides its output. */
interface Building {
  feedbacks: Feedback[];
  buffers: number[];
  writes: Array<{ buffer: number; index: Operand; value: Operand }>;
  watches: Array<{ name: string; value: Operand }>;
  numbers: Array<{ name: string; value: Operand }>;
  bangs: Array<{ name: string; value: Operand }>;
  notes: Array<{ name: string; gate: Operand; pitch: Operand; velocity: Operand }>;
}

let current: Building | undefined;

function building(): Building {
  if (!current) throw new Error("feedback, buffer, write, watch and output belong inside `sound`");
  return current;
}

/** The sample that comes in, in an effect. 0 in an instrument or a source. */
export const input: Signal = new Named({ op: "input" });
/** Both channels of what comes in, for a sound that returns `{ left, right }`. */
export const inputLeft: Signal = new Named({ op: "inputLeft" });
export const inputRight: Signal = new Named({ op: "inputRight" });

/**
 * A sound in stereo: it runs once per sample for both channels and hears both, for a ping-pong,
 * a panner or a widener. A sound that returns one signal runs on each channel apart.
 */
export interface Stereo {
  left: Operand;
  right: Operand;
}
/** 0 on the left channel, 1 on the right. */
export const channel: Signal = new Named({ op: "channel" });
export const sampleRate: Signal = new Named({ op: "sampleRate" });
/** Quarter notes since the start of the piece, while it plays; it stands still while stopped. */
export const beat: Signal = new Named({ op: "beat" });
export const bpm: Signal = new Named({ op: "bpm" });
/** 1 while the piece plays. */
export const playing: Signal = new Named({ op: "playing" });
/** The note of the voice. */
export const note: {
  /** In Hz, with the bend wheel. */
  freq: Signal;
  /** As a MIDI number: 69 is A4. */
  pitch: Signal;
  /** 1 while held, 0 after. */
  gate: Signal;
  /** How hard it was played, 0 to 1. */
  velocity: Signal;
  /** 1 in the first sample of the note. */
  onset: Signal;
} = {
  freq: new Named({ op: "frequency" }),
  pitch: new Named({ op: "pitch" }),
  gate: new Named({ op: "gate" }),
  velocity: new Named({ op: "velocity" }),
  onset: new Named({ op: "onset" }),
};
export const PI = Math.PI;
export const TAU = 2 * Math.PI;

const unary =
  (op: Unary) =>
  (x: Operand): Signal =>
    new Signal({ op, x });
const binary =
  (op: Binary) =>
  (a: Operand, b: Operand): Signal =>
    new Signal({ op, a, b });
export const sin = unary("sin");
export const cos = unary("cos");
export const tan = unary("tan");
export const tanh = unary("tanh");
export const abs = unary("abs");
export const sqrt = unary("sqrt");
export const exp = unary("exp");
export const log = unary("log");
export const floor = unary("floor");
/** The part after the point: 0 to 1. */
export const wrap = unary("wrap");
/** The gain of a level in dB: `db(-6)` is about 0.5. */
export const db: (decibels: Operand) => Signal = unary("db");
/** Clean up to full scale, then bends softly; never above 1.5. */
export const saturate = unary("saturate");
export const min = binary("min");
export const max = binary("max");
export const pow: (x: Operand, power: Operand) => Signal = binary("pow");
export const clamp = (x: Operand, low: Operand, high: Operand): Signal =>
  new Signal({ op: "clamp", x, low, high });
/** `a` at 0, `b` at 1, in between for amounts in between. */
export const mix = (a: Operand, b: Operand, amount: Operand): Signal =>
  new Signal({ op: "mix", a, b, amount });
/** A ramp from 0 to 1, `hz` times a second. `sin(phasor(hz).times(TAU))` is a sine. */
export const phasor = (hz: Operand): Signal => new Signal({ op: "phasor", hz });
/** White noise from -1 to 1, different on each channel. */
export const noise = (): Signal => new Signal({ op: "noise" });
/**
 * `x` as it was `ms` milliseconds ago, up to 4000. `ms` may move every sample, smoothly, for a
 * chorus or a tape wobble. `longest` is the most it holds, a number; give it in an instrument,
 * where every voice keeps its own.
 */
export const delay = (x: Operand, ms: Operand, longest?: number): Signal =>
  new Signal(longest === undefined ? { op: "delay", x, ms } : { op: "delay", x, ms, longest });
const filter =
  (op: "lowpass" | "highpass" | "bandpass") =>
  (x: Operand, hz: Operand, q: Operand = Math.SQRT1_2): Signal =>
    new Signal({ op, x, hz, q });
/** Filters. `q` is 0.707 when left out: no peak; higher rings at `hz`, up to 20. */
export const lowpass = filter("lowpass");
export const highpass = filter("highpass");
export const bandpass = filter("bandpass");
/** `x` that follows changes slowly, in about `ms` milliseconds. */
export const smooth = (x: Operand, ms: Operand): Signal => new Signal({ op: "smooth", x, ms });
/** An envelope from 0 to 1 that follows `gate`; times in ms, `sustain` 0 to 1. */
export const adsr = (
  gate: Operand,
  attack: Operand,
  decay: Operand,
  sustain: Operand,
  release: Operand,
): Signal => new Signal({ op: "adsr", gate, attack, decay, sustain, release });
/** 1 in the sample where `x` goes from 0 or below to above 0: a clock from a ramp. */
export const rise = (x: Operand): Signal => new Signal({ op: "rise", x });
/** 1 in the sample where `x` differs from the sample before. */
export const change = (x: Operand): Signal => new Signal({ op: "change", x });
/** `x` as it was the last time `when` was above 0: sample and hold. */
export const hold = (x: Operand, when: Operand): Signal => new Signal({ op: "hold", x, when });
/** A table read from `phase` 0 to 1 over its length, wrapped, smoothly between its values. */
export const lookup = (table: Table, phase: Operand): Signal =>
  new Signal({ op: "lookup", table: table.ref, phase });

/** A sine at `hz`, from -1 to 1. */
export const sine = (hz: Operand): Signal => sin(phasor(hz).times(TAU));
/** A saw at `hz`, from -1 to 1. */
export const saw = (hz: Operand): Signal => phasor(hz).times(2).minus(1);
/** A square at `hz`, from -1 to 1, up for `width` of each cycle (0.5 when left out). */
export const square = (hz: Operand, width: Operand = 0.5): Signal =>
  phasor(hz).lt(width).times(2).minus(1);
/** A triangle at `hz`, from -1 to 1. */
export const triangle = (hz: Operand): Signal =>
  abs(phasor(hz).times(4).minus(2)).minus(1);
/** The frequency of a MIDI pitch: `mtof(69)` is 440 Hz. */
export const mtof = (pitch: Operand): Signal =>
  pow(2, new Signal({ op: "subtract", a: pitch, b: 69 }).over(12)).times(440);

/** A device a sound plays through: what it hears, and its record. */
export interface DeviceCall {
  tool: string;
  input: [Operand, Operand];
  numbers: Record<string, Operand>;
  state: Record<string, unknown>;
}

/**
 * What a device plays: as a signal, of the channel the sound runs on, and of each channel with
 * `left` and `right`. A sound that returns `{ left, right }` reads those.
 */
export class DeviceSound extends Signal {
  readonly left: Signal;
  readonly right: Signal;
  constructor(call: DeviceCall) {
    super({ op: "played", device: call });
    this.left = new Signal({ op: "played", device: call, channel: 0 });
    this.right = new Signal({ op: "played", device: call, channel: 1 });
  }
}

/** The settings of a device: its record `R`, with a signal or a number for each of its numbers `F`. */
export type DeviceSettings<F extends Fields, R> = { [Name in keyof R]?: Name extends keyof F ? Operand : R[Name] };

/** A built-in device to play inside `sound`, such as `devices.reverb`. */
export interface Device<F extends Fields, R> {
  /** `x` through the device, at its record's defaults where `settings` says nothing. */
  (x: Operand | Stereo, settings?: DeviceSettings<F, R>): DeviceSound;
  /** Its numbers as knobs, for the `state` of a tool: `{ ...devices.reverb.fields }`. */
  readonly fields: F;
}

/** For the devices below. */
function device<const T extends keyof Tools, const F extends Fields>(tool: T, fields: F): Device<F, StateOfTool<T>> {
  const play = (x: Operand | Stereo, settings: Record<string, unknown> = {}) => {
    const numbers: Record<string, Operand> = {};
    const state: Record<string, unknown> = {};
    for (const [name, value] of Object.entries(settings)) {
      if (name in fields && isOperand(value)) numbers[name] = value;
      else state[name] = value;
    }
    const input: [Operand, Operand] = isOperand(x) ? [x, x] : [x.left, x.right];
    return new DeviceSound({ tool, input, numbers, state });
  };
  return Object.assign(play, { fields }) as Device<F, StateOfTool<T>>;
}

/** `fields` with `prefix` before each name, so a tool can have the numbers of two devices. */
export function prefixed<const P extends string, const F extends Fields>(
  prefix: P,
  fields: F,
): { [Name in keyof F as `${P}${Name & string}`]: F[Name] } {
  return Object.fromEntries(Object.entries(fields).map(([name, field]) => [prefix + name, field])) as never;
}

/** A value that feeds back, see `Feedback`. Inside `sound` only. */
export function feedback(): Feedback {
  const made = new Feedback(building().feedbacks.length);
  building().feedbacks.push(made);
  return made;
}

/** Memory of `seconds`, see `Buffer`. Inside `sound` only. */
export function buffer(seconds: number): Buffer {
  const made = new Buffer(building().buffers.length);
  building().buffers.push(seconds);
  return made;
}

/** Shows `value` to the card as `watches[name]`. Inside `sound` only. */
export function watch(name: string, value: Operand): void {
  building().watches.push({ name, value });
}

/** Notes from signals: one sounds while `gate` is above 0, at the MIDI `pitch` it starts with. */
export interface Notes {
  gate: Operand;
  pitch: Operand;
  /** 0 to 1, as it starts; 0.8 when left out. */
  velocity?: Operand;
  bang?: never;
}

/**
 * Sends out what a parent connects to another child: a number, in the units of the number it
 * drives; `{ bang }`, a bang where it goes above 0; or `Notes`, a note that ends where the gate
 * falls or the pitch moves. Inside `sound` only; an instrument sends only numbers.
 */
export function output(name: string, value: Operand | { bang: Operand; gate?: never } | Notes): void {
  const declared = building();
  if (typeof value === "number" || value instanceof Signal) {
    declared.numbers.push({ name, value });
  } else if (value.bang === undefined) {
    declared.notes.push({ name, gate: value.gate, pitch: value.pitch, velocity: value.velocity ?? 0.8 });
  } else {
    declared.bangs.push({ name, value: value.bang });
  }
}

/**
 * For the host: the graph of a sound. `build` makes the output; what it declares is kept
 * meanwhile. A signal is one node, after the nodes it reads; a number or a `Named` is a node
 * of its own wherever it is read.
 */
export function graphToJson(build: () => Operand | Stereo): Graph {
  current = { feedbacks: [], buffers: [], writes: [], watches: [], numbers: [], bangs: [], notes: [] };
  try {
    const declared = current;
    const output = build();
    const nodes: GraphNode[] = [];
    const made = new Map<Signal, number>();
    const calls = new Map<DeviceCall, number>();
    const add = (node: GraphNode) => nodes.push(node) - 1;
    /** The node of a device, made once, after what it hears and the signals of its numbers. */
    const device = (call: DeviceCall): number => {
      const known = calls.get(call);
      if (known !== undefined) return known;
      const input = call.input.map(use) as [number, number];
      const numbers = Object.fromEntries(Object.entries(call.numbers).map(([name, value]) => [name, use(value)]));
      const index = add({ op: "device", tool: call.tool, input, numbers, state: call.state });
      calls.set(call, index);
      return index;
    };
    /** Adds the nodes of `operand` that are made once, the signals it reads first. */
    const prepare = (operand: Operand) => {
      if (typeof operand === "number" || operand instanceof Named || made.has(operand)) return;
      if (operand.node.op === "played") {
        const { device: call, channel } = operand.node;
        made.set(operand, add({ op: "played", device: device(call), ...(channel === undefined ? {} : { channel }) }));
        return;
      }
      const operands = Object.entries(operand.node).filter((entry): entry is [string, Operand] =>
        isOperand(entry[1]),
      );
      for (const [, each] of operands) prepare(each);
      const read = Object.fromEntries(operands.map(([key, each]) => [key, use(each)]));
      made.set(operand, add({ ...operand.node, ...read } as GraphNode));
    };
    /** The node of `operand` where it is used, made now when it is a number or a `Named`. */
    const use = (operand: Operand): number => {
      prepare(operand);
      if (typeof operand === "number") return add({ op: "constant", value: quantize(operand) });
      return operand instanceof Named ? add(operand.node) : (made.get(operand) as number);
    };
    const outputs = typeof output === "object" && !(output instanceof Signal) ? [output.left, output.right] : [output];
    outputs.forEach(prepare);
    const watches = declared.watches.map(({ name, value }) => ({ name, node: use(value) }));
    const numbers = declared.numbers.map(({ name, value }) => ({ name, node: use(value) }));
    const bangs = declared.bangs.map(({ name, value }) => ({ name, node: use(value) }));
    const notes = declared.notes.map(({ name, gate, pitch, velocity }) => ({
      name,
      gate: use(gate),
      pitch: use(pitch),
      velocity: use(velocity),
    }));
    for (const { buffer, index, value } of declared.writes) {
      prepare(index);
      prepare(value);
      add({ op: "write", buffer, index: use(index), value: use(value) });
    }
    const feedbacks = declared.feedbacks.map((feedback) => {
      if (feedback.value === undefined) {
        throw new Error("a feedback is never set: call .set(value) on it");
      }
      return use(feedback.value);
    });
    const [first, second] = outputs.map(use);
    return {
      nodes,
      feedbacks,
      buffers: declared.buffers.map(quantize),
      watches,
      numbers,
      bangs,
      notes,
      output: second === undefined ? { mono: first } : { left: first, right: second },
    };
  } finally {
    current = undefined;
  }
}

function isOperand(value: unknown): value is Operand {
  return typeof value === "number" || value instanceof Signal;
}

/** A number as the sound plays it: to 9 decimals, and 0 below that. */
function quantize(value: number): number {
  if (!Number.isFinite(value)) {
    throw new Error(`${value} is not a number a sound can play`);
  }
  return Math.abs(value) < 1e-9 ? 0 : Number(value.toFixed(9));
}

// ---------------------------------------------------------------------------------------------
// Tools

export interface ToolSpec<S extends Fields, C extends Controls, M, K extends ChildTools = ChildTools> {
  /** The `tool` of its records and the name of its doc: lowercase letters, digits, `-`, `_`. */
  name: string;
  /** What the card and the picker call it. */
  title: string;
  /** One line for the map of docs: when an agent should read the doc. */
  when: string;
  /** What it does and how its fields change the sound, in Markdown. Goes into its doc. */
  doc: string;
  /**
   * `effect` (the default) goes in a track's effects and reads `input`. `instrument` is what a
   * track plays: the sound runs once per note. `source` is what a track plays too, one voice
   * that runs all the time and follows the newest note; at the top of a project, connected to
   * the device in `project.json`, it is an experiment of its own.
   */
  kind?: "effect" | "instrument" | "source";
  /** The fields of its record. Field names are lowercase letters, digits and `_`. */
  state: S;
  /** What its card plays and nothing saves. Names are as field names. */
  controls?: C;
  /**
   * Its sound: a `Signal` made with the functions of the sound graph, or a `Stereo` pair.
   * `input` when left out, which passes the sound through.
   */
  sound?: (fields: SoundOf<S, C>) => Operand | Stereo;
  /**
   * Makes it a parent: its record is `state/<name>/instance.json` and the records next to it
   * are its children. A pure function of its record and its children that says how they and
   * its own ports connect. It runs in Bun when either changes, and what it declares plays a
   * moment later; the sound plays on meanwhile. Its own `sound` hears what it connects to its
   * own `{ port: "audio" }` as `input`, and is its `audio` output.
   */
  behaviour?: (parent: Parent<S, K>) => Declaration<K>;
  /**
   * The children it has, by name, and the tool of each: `{ bass: "sequencer", lead: drone }`,
   * where `drone` is what `tool` returned for a tool of this project. TypeScript then checks
   * every connection, `set`, `fire`, `play`, `send` and edit of them against their tools, and a
   * child of another tool, a missing one or one it does not name is a problem. Leave it out for
   * children whose names a record decides.
   */
  children?: K & LiteralTools<K>;
  /**
   * What the control loop and the card of an instance keep, and nothing saves: positions of
   * a simulation, the last thing played. Made new for each instance, and when the code loads.
   */
  memory?: () => M;
  /**
   * The control loop: about 30 times a second while the window is open, for each instance. It
   * plays the sound with `set` and `fire`, as a performer would, and keeps what it needs in
   * `memory`.
   */
  tick?: (tool: Tick<ValuesOf<S>, C, M, K>) => void;
  /** Its card. Without one it gets a knob per knob and a button per option and toggle. */
  card?: (card: Context<ValuesOf<S>, C, M, K>) => Node;
  /** Its page: the whole window, for an instance at the top of a project with no arrangement. */
  page?: (page: Page<ValuesOf<S>, C, M, K>) => Node;
  /**
   * Its page shows the transport (play, stop, position, tempo, time signature, click), and its
   * card and page draw again as `transport` moves. Recording is the arrangement's.
   */
  transport?: boolean;
  /**
   * A key of the computer keyboard goes down or up, while its page shows or after a click on
   * its card. Plain keys are then the tool's: space and `r` do not play or record.
   */
  onKey?: (tool: Context<ValuesOf<S>, C, M, K>, key: Key) => void;
  /**
   * Hears what reaches its `notes`, whatever plays them: the keyboard, a clip, a cable, a
   * parent's `play` and `send`; not its own. Its sound plays them by itself as well.
   */
  onNotes?: (tool: Context<ValuesOf<S>, C, M, K>, event: HeardEvent) => void;
}

/** A record and the records of its children: what a behaviour gets, and what `update` changes. */
export interface Part<State, K extends ChildTools = ChildTools> {
  state: State;
  /** The records next to its own, in name order. */
  children: OwnedChild<K>[];
}

/** What a behaviour gets. */
export type Parent<S extends Fields, K extends ChildTools = ChildTools> = Frozen<Part<ValuesOf<S>, K>>;

/** The name of a tool: a built-in one, which `Tools` describes, or one of this project. */
export type ToolName = keyof Tools | (string & {});

declare const entry: unique symbol;

/**
 * A tool of this project as `tool` returns it: its name, which is what the records hold, typed
 * with an entry as `Tools` has one. Named in a parent's `children`, its `state` and inputs are
 * checked; a name typed as a string checks nothing.
 */
export type ProjectTool<Name extends string = string, Entry = unknown> = Name & { readonly [entry]: Entry };

/** The entry of `T`: in `Tools`, or of a `ProjectTool`. */
type EntryOf<T> = T extends keyof Tools ? Tools[T] : T extends ProjectTool<string, infer Entry> ? Entry : {};

/** The tools of the children of a parent by their names, as its `children` names them. */
export type ChildTools = Record<string, ToolName>;

/** The `state` of a record of `T` as the file holds it; any value for a tool named by a string. */
export type StateOfTool<T> = EntryOf<T> extends { state: infer State } ? State : unknown;

/** `children` as written. A tool name widened to `string` would check nothing, so it is refused. */
type LiteralTools<K extends ChildTools> = {
  [Name in keyof K]: string extends K[Name] ? "the tool names were widened to string: write the children as `const children = { ... } as const` and give the behaviour the return type `Declaration<typeof children>`, or leave the return type out" : K[Name];
};

/** The names of its children. */
type ChildName<K extends ChildTools> = keyof K & string;

/** A child of a parent: its record. */
export type OwnedChild<K extends ChildTools = ChildTools> = {
  [Name in ChildName<K>]: {
    /**
     * Its file name without `.json`: lowercase letters, digits, `-` and `_`. It is the id of the
     * record, which never changes: keep a title in a field.
     */
    readonly name: Name;
    /** Its `tool`, such as `"instrument.synth"`. */
    tool: K[Name];
    /** Its `state` as the file holds it, such as a `SequencerState`. */
    state: StateOfTool<K[Name]>;
  };
}[ChildName<K>];

/** The keys that name a port, one for each kind of what it carries. */
type PortKey = "port" | "notes" | "number" | "bang" | "live" | "trigger";

/** A port named under the key `K` and no other, so a connection joins two of one kind. */
type Keyed<K extends PortKey, Name extends string = string> = Record<K, Name> & { [Other in Exclude<PortKey, K>]?: never };
/**
 * The names a child of tool `T` has under the key `P`, as its entry says: any where it says
 * none. Any output (`Out`) of a tool of this project too, as its `sound` declares them.
 */
type PortName<T, P extends PortKey, Out extends boolean> = [Out, T] extends [true, ProjectTool]
  ? string
  : P extends keyof EntryOf<T>
    ? EntryOf<T>[P]
    : string;
/** A port of a child under the key `P`, of a name its tool has as an input, or as an output with `Out`. */
type PortOf<P extends PortKey, K extends ChildTools, Out extends boolean = false> = {
  [Name in ChildName<K>]: { child: Name } & Keyed<P, PortName<K[Name], P, Out> & string>;
}[ChildName<K>];
/** A port of the parent itself under the key `K`: no `child`. */
type OwnOf<K extends PortKey, Name extends string = string> = { child?: never } & Keyed<K, Name>;

/** A sound port of a child: `audio` when left out. An effect has an `audio` input and output. */
export type ChildPort<K extends ChildTools = ChildTools> = Partial<Keyed<"port">> & { child: ChildName<K> };
/** Notes of a child: its input `notes`, such as an instrument's, or with `Out` notes it sends out. */
export type ChildNotes<K extends ChildTools = ChildTools, Out extends boolean = false> = PortOf<"notes", K, Out>;
/**
 * A number of a child: a knob it takes, such as the knob `cutoff_hz` of a `filter` or a knob of
 * a tool of the project, or with `Out` one it sends out with `output`.
 */
export type ChildNumber<K extends ChildTools = ChildTools, Out extends boolean = false> = PortOf<"number", K, Out>;
/** A bang a child sends out with `output`. */
export type ChildBang<K extends ChildTools = ChildTools> = PortOf<"bang", K, true>;
/** A live control of a child, or another number it plays from, such as `launch` of a `sequencer`. */
export type ChildLive<K extends ChildTools = ChildTools> = PortOf<"live", K>;
/** A trigger of a child. */
export type ChildTrigger<K extends ChildTools = ChildTools> = PortOf<"trigger", K>;

/**
 * The parent's own sound port. From it, what reaches the parent's `audio` input; to it, what the
 * parent plays out of its `audio` output, through its own `sound`.
 */
export type OwnAudio = OwnOf<"port", "audio">;
/** What reaches the parent's own `notes`: the MIDI keyboard at the top, its track's notes in a track. */
export type OwnNotes = OwnOf<"notes", "notes">;

/**
 * Joins an output of a child, or what reaches an input of the parent, to an input of a child or
 * an output of the parent, of one kind. In `to`, a `notes`, `number` or `bang` without `child`
 * is an output of the parent of that name, which passes what reaches it on to its parent.
 */
export type Connection<K extends ChildTools = ChildTools> =
  | { from: ChildPort<K> | OwnAudio; to: ChildPort<K> | OwnAudio }
  | { from: ChildNotes<K, true> | OwnNotes; to: ChildNotes<K> | OwnOf<"notes"> }
  | { from: ChildNumber<K, true>; to: ChildNumber<K> | ChildLive<K> | OwnOf<"number"> }
  | { from: ChildBang<K>; to: ChildTrigger<K> | OwnOf<"bang"> };

/** What a behaviour declares. A JSON value: no functions, no classes. */
export interface Declaration<K extends ChildTools = ChildTools> {
  connections: Connection<K>[];
}

/**
 * A key of the computer keyboard: its name, such as `"a"`, `"1"`, `"space"` or `"left"`, and the
 * modifiers held. A key with cmd, ctrl or alt only goes down, such as cmd-c; one the window
 * uses, such as cmd-z, stays the window's.
 */
export interface Key extends Modifiers {
  key: string;
  /** Pressed; `false` when it comes up. */
  down: boolean;
}

/**
 * What a `notes` cable carries, `send` plays and `onNotes` hears. `pitch` is MIDI, 69 is A4; a
 * velocity or a value is 0 to 1; the bend is -1 to 1. `cc` is a MIDI controller, 0 to 119: the
 * sustain pedal is 64, down from 0.5, and the mod wheel 1. `allOff` ends every note, puts the
 * pedal and the wheels at rest, as a stop does, and drops the notes sent before it to play after it.
 */
export type NoteEvent =
  | { type: "noteOn"; pitch: number; velocity: number }
  | { type: "noteOff"; pitch: number }
  | { type: "cc"; controller: number; value: number }
  | { type: "bend"; value: number }
  | { type: "pressure"; value: number }
  | { type: "allOff" };

/** A note event as `onNotes` hears it: `at` is the `time` it sounded at, to the sample. */
export type HeardEvent = NoteEvent & { at: number };

/** The names of the knobs in `S`, the numbers a cable turns. */
type KnobNames<S extends Fields> = { [Name in keyof S]: S[Name] extends KnobField ? Name : never }[keyof S] & string;

/** The entry of a tool of the project with fields `S` and controls `C`. */
type ProjectEntry<S extends Fields, C extends Controls> = {
  state: StateOf<S>;
  number: KnobNames<S>;
  live: ControlNames<C, "live">;
  trigger: ControlNames<C, "trigger">;
  notes: "notes";
};

/** Makes a tool of the project. Export what it returns for a parent to name in its `children`. */
export function tool<
  const Name extends string,
  const S extends Fields,
  const C extends Controls = {},
  M = {},
  const K extends ChildTools = ChildTools,
>(spec: ToolSpec<S, C, M, K> & { name: Name }): ProjectTool<Name, ProjectEntry<S, C>> {
  if (host.tools.has(spec.name)) {
    throw new Error(`a tool named ${spec.name} is already defined`);
  }
  host.tools.set(spec.name, spec as unknown as ToolSpec<Fields, Controls, unknown>);
  host.stacks.set(spec.name, new Error().stack ?? "");
  return spec.name as ProjectTool<Name, ProjectEntry<S, C>>;
}

/**
 * What plays an instance as a performer would: nothing saves it and no undo takes it back.
 * `at` is a `time` of the control loop, at most 10 s ahead: what has one happens on that
 * sample, or at once when it has passed. A parent plays a child the same way, by its name: `ChildLive`,
 * `ChildTrigger`, or `child` among the options of `play` and `send`.
 */
export interface Performer<C extends Controls, K extends ChildTools = ChildTools> {
  /** Moves a live control. */
  set(control: ControlNames<C, "live"> | ChildLive<K>, value: number): void;
  /** Fires a trigger. */
  fire(control: ControlNames<C, "trigger"> | ChildTrigger<K>, options?: { at?: number }): void;
  /**
   * Sends a note event to the tool's own voices, or into the `notes` of `child`. An instrument
   * plays each note on a voice of its own; a source follows the newest. At most 256 wait for
   * their `at`: past that a `noteOn` is dropped, and anything else plays at once.
   */
  send(event: NoteEvent, options?: { at?: number; child?: ChildName<K> }): void;
  /**
   * Sends a `noteOn` and its `noteOff` `seconds` later (0.25 when left out), or with `hold`
   * only the `noteOn`. `velocity` is 0.8 when left out.
   */
  play(
    pitch: number,
    options?: { seconds?: number; hold?: boolean; velocity?: number; at?: number; child?: ChildName<K> },
  ): void;
}

/** A value code reads and does not change in place: `update` changes a record. */
export type Frozen<T> = T extends string | number | boolean | null | undefined
  ? T
  : T extends readonly (infer Item)[]
    ? readonly Frozen<Item>[]
    : T extends object
      ? { readonly [Key in keyof T]: Frozen<T[Key]> }
      : T;

/** What all code of an instance gets: its card, its page, `tick`, `onKey` and `onNotes`. */
export interface Context<State, C extends Controls = Controls, M = unknown, K extends ChildTools = ChildTools>
  extends Performer<C, K> {
  /** The record as it is now, every field at its default where the record leaves it out. */
  state: Frozen<State>;
  /** The records of its children, as its behaviour gets them. */
  children: Frozen<OwnedChild<K>[]>;
  /**
   * Changes the record and those of its children: `change` gets a copy of the latest to edit,
   * also after an `await`, and may change a child's `state`, push a new child or leave one
   * out. What the records do not accept is refused whole, and listed in `problems.txt`. Updates
   * of one `label` in a row are one undo step.
   */
  update(label: string, change: (part: Part<State, K>) => void): void;
  /** What its code keeps and nothing saves. A card draws again after a click. */
  memory: M;
  /** The last value of each `watch` of the sound, by name. A card draws again as they move. */
  watches: Record<string, number>;
  /** The clock of the sound, in seconds: where it is now. Play `at` a little after it. */
  time: number;
  /** Where the piece plays. A card draws again as it moves only when the tool sets `transport: true`. */
  transport: Transport;
  /** The sources of live values: what a card shows moving without drawing again. */
  now: Now<State, C>;
}

/** What the control loop of an instance gets each time it runs. */
export interface Tick<State, C extends Controls = Controls, M = unknown, K extends ChildTools = ChildTools>
  extends Context<State, C, M, K> {
  /** Seconds since the last tick. */
  dt: number;
}


// ---------------------------------------------------------------------------------------------
// Cards and pages: a tree the code draws, which the window shows. What moves on it (a drag, a
// playhead, a meter, a knob turned) moves in the window, without running code: a live value.

/** A colour as `#rrggbb` or `#rrggbbaa`. */
export type Color = `#${string}`;

/** How a `div` lays out and looks. Numbers are points. */
export interface Style {
  /** How children line up. A div is a column unless this says `row`. */
  direction?: "row" | "column";
  gap?: number;
  padding?: number;
  paddingX?: number;
  paddingY?: number;
  width?: number;
  height?: number;
  /** Takes the room left in its parent. */
  grow?: boolean;
  /** Where children sit across the direction. */
  align?: "start" | "center" | "end";
  /** Where children sit along the direction. */
  justify?: "start" | "center" | "end" | "between";
  background?: Color;
  color?: Color;
  fontSize?: number;
  radius?: number;
  borderColor?: Color;
  borderWidth?: number;
}

/** Where the project plays, as the window shows it. */
export interface Transport {
  playing: boolean;
  /** The position in ticks, 960 to a quarter note. */
  tick: number;
  /** The bar and the beat of `tick`, from 1. */
  bar: number;
  beat: number;
  /** The tempo at `tick`, in beats per minute. */
  bpm: number;
}

/** What a page gets: what all code gets, and the room it has. */
export interface Page<State, C extends Controls = Controls, M = unknown, K extends ChildTools = ChildTools>
  extends Context<State, C, M, K> {
  /** Its width and height in points. It draws again when the window changes. */
  size: { width: number; height: number };
}

// Live values

/** What a live value is made of, as the window gets it. */
type LiveNode =
  | { op: "tick" }
  | { op: "watch"; name: string }
  | { op: "field" | "control"; name: string; fraction: boolean }
  | { op: "drag"; id: string; part: keyof DragValues }
  | { op: "space"; key: string; part: keyof SpaceValues }
  | { op: "plus" | "minus" | "times" | "over" | "min" | "max" | "lt" | "gt" | "le" | "ge" | "eq" | "ne" | "snap"; a: Num; b: Num }
  | { op: "negate" | "abs" | "floor" | "round" | "sin" | "cos"; x: Num };

/**
 * A number the window works out again on every frame, without running code: where the
 * playhead is, a watch, a field as a drag turns it, how far a drag went. Combine them with the
 * methods, which take a live value or a number. Code cannot read one: it is no number, so
 * `if (now.tick > 960)` is a type error. A card draws again only when what its code read changed.
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
  /** 1 while the pointer holds it. */
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

/** The sources of live values, which all code gets as `now`. */
export interface Now<State = Record<string, unknown>, C extends Controls = Controls> {
  /** Where the piece plays, in ticks: it moves on every frame as it plays. */
  tick: Live;
  /** The last value of a watch of the sound. */
  watch(name: string): Live;
  /**
   * A number of the record as it is now, also as a drag turns it. With `fraction`, how far
   * along the travel of its knob it is, 0 to 1, on a log scale for `"hz"`.
   */
  field(name: NumberNames<State>, options?: { fraction?: boolean }): Live;
  /** Where a live control was put last; with `fraction`, 0 to 1 over its range. */
  control(name: ControlNames<C, "live">, options?: { fraction?: boolean }): Live;
  /** The drag of `id`, see `Drag`. */
  drag(id: string): DragValues;
  /** The space of `key`, see `Space`. */
  space(key: string): SpaceValues;
}

function sources(): Now<Record<string, number>> {
  const part = <Part extends string>(make: (part: Part) => LiveNode) => (parts: readonly Part[]) =>
    Object.fromEntries(parts.map((each) => [each, new Live(make(each))])) as Record<Part, Live>;
  return {
    tick: new Live({ op: "tick" }),
    watch: (name) => new Live({ op: "watch", name }),
    field: (name, options) => new Live({ op: "field", name, fraction: options?.fraction ?? false }),
    control: (name, options) => new Live({ op: "control", name, fraction: options?.fraction ?? false }),
    drag: (id) =>
      part<keyof DragValues>((each) => ({ op: "drag", id, part: each }))(["startX", "startY", "dx", "dy", "x", "y", "active"]),
    space: (key) =>
      part<keyof SpaceValues>((each) => ({ op: "space", key, part: each }))(["left", "top", "right", "bottom", "pointX", "pointY"]),
  };
}

/** The sources of live values, for the components of a card, which name fields by strings. */
export const now: Now<Record<string, number>> = sources();

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

/** A drag at its end, or at a move with `onMove`: as `DragValues` says, in numbers. */
export interface DragResult extends Modifiers {
  startX: number;
  startY: number;
  dx: number;
  dy: number;
  x: number;
  y: number;
}

/**
 * A number a drag turns as it goes, in the window, with no code: a knob or a fader. The whole
 * drag is one undo step for a `field`; a `control` is played, not saved. Without `min` and `max`
 * it takes the field's or the control's.
 */
export type DragTurns = ({ field: string } | { control: string }) & {
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
 * Until its next tree after `onEnd` arrives, the drag's values stay, so what moved does not
 * jump back while its code runs.
 */
export interface Drag {
  /** Names its live values: `now.drag(id)`. */
  id?: string;
  /**
   * Rounds `dx` and `dy` to whole steps, in units. With `grid`, the pressed shape's `anchor` lands
   * on a whole step instead, as a note lands on the lines of a piano roll wherever it started.
   */
  snap?: { x?: number; y?: number; grid?: boolean };
  /** Moves along one axis only. */
  axis?: "x" | "y";
  /** What the drag turns across and up. */
  x?: DragTurns;
  y?: DragTurns;
  /** At the end of the drag, when the pointer moved. */
  onEnd?: (drag: DragResult) => void;
  /**
   * At every move: runs code each time, so what it draws is a moment behind the pointer. For
   * drawing with the pointer, as on paper; a move or a resize is `now.drag` and `onEnd`.
   */
  onMove?: (drag: DragResult) => void;
}

/** What a shape the pointer reaches can do. */
interface Pointed {
  /** Keeps its hover and its drag when the tree is drawn again. */
  key?: string;
  onPress?: (press: Press) => void;
  drag?: Drag;
  /** Its colour while the pointer is over it. */
  hover?: Color;
  cursor?: "pointer" | "grab" | "crosshair" | "ew-resize" | "ns-resize";
  /** 0 to 1; 1 when left out. */
  opacity?: Num;
  /** The point a drag with `snap.grid` puts on the grid: its own `x` and `y` when left out, such as the end of a note for its resize handle. */
  anchor?: { x?: number; y?: number };
}

/**
 * Something a space draws. Positions and sizes are in the units of the space; `radius`, `width`
 * of a line and `size` of text are in points.
 */
export type Shape =
  /** Shapes moved together by `x` and `y`, such as the selected notes by a drag: one live value for all of them. */
  | { kind: "group"; x?: Num; y?: Num; children: Child[] }
  | (Pointed &
  (
    | { kind: "rect"; x: Num; y: Num; width: Num; height: Num; color: Color; radius?: number; border?: Color; borderWidth?: Num }
    | { kind: "circle"; x: Num; y: Num; radius: Num; color: Color }
    | { kind: "line"; from: [Num, Num]; to: [Num, Num]; color: Color; width?: number }
    /** Along a circle from one angle to another, in turns clockwise from the top: a dial. */
    | { kind: "arc"; x: Num; y: Num; radius: number; from: Num; to: Num; color: Color; width?: number }
    /** Text at `x`, `y`, its top; a live number shows as a readout in `unit`. */
    | { kind: "text"; x: Num; y: Num; text: string | Live; unit?: Unit; color?: Color; size?: number; align?: "left" | "center" | "right" }
  ));

/**
 * A surface that draws shapes in units of its own, such as ticks across and notes down. It
 * scrolls over `extent` and zooms in `zoom` in the window, with no code, and draws only what
 * shows. Without `unit`, a unit is a point.
 */
export interface SpaceProps {
  /** Names it for `now.space(key)`, and keeps its scroll and zoom when the tree is drawn again. */
  key?: string;
  /** Its size in points; `grow` takes the room left in its parent. */
  width?: number;
  height?: number;
  grow?: boolean;
  /** Points per unit at zoom 1. */
  unit?: { x?: number; y?: number };
  /** What it scrolls over, in units; an axis without one does not scroll, and shows from 0. */
  extent?: { x?: readonly [number, number]; y?: readonly [number, number] };
  /** How far it zooms in and out, as a factor; an axis without one does not zoom. Cmd and scroll, or a pinch. */
  zoom?: { x?: readonly [number, number]; y?: readonly [number, number] };
  /** Where it is scrolled to when it first draws, in units; the window keeps where it is after. */
  scroll?: { x?: number; y?: number };
  /**
   * The key of a space whose scroll and zoom it shows on an axis, and moves as it scrolls: the
   * ruler and the keys of a piano roll. Give it the same `unit`, `extent` and `zoom` on that axis.
   */
  follow?: { x?: string; y?: string };
  background?: Color;
  radius?: number;
  /** A press where no shape that hears the pointer is. */
  onPress?: (press: Press) => void;
  /** A drag from where no shape that hears the pointer is, such as a marquee. */
  drag?: Drag;
  cursor?: Pointed["cursor"];
  children?: Child;
}

export type Child = Node | string | number | boolean | null | undefined | Child[];

/** What a card draws. A `Shape` is one too, for JSX, but only a `Space` draws one. */
export type Node =
  | Shape
  | { type: "div"; key?: string; style?: Style; onClick?: (press: Press) => void; children: Child[] }
  | { type: "sample"; path: string; label?: string }
  | ({ type: "space" } & Omit<SpaceProps, "children"> & { children: Child[] });

/** A space, see `SpaceProps`. */
export const Space = ({ children, ...props }: SpaceProps): Node => ({ type: "space", ...props, children: [children] });

type ShapeOf<Kind extends Shape["kind"]> = Omit<Extract<Shape, { kind: Kind }>, "kind">;
const shape =
  <Kind extends Shape["kind"]>(kind: Kind) =>
  (props: ShapeOf<Kind>) =>
    ({ kind, ...props }) as unknown as Shape;
export const Rect = shape("rect");
export const Circle = shape("circle");
export const Line = shape("line");
export const Arc = shape("arc");
export const Label = shape("text");
/** Shapes moved together, see `Shape`. */
export const Group = ({ children, ...props }: { x?: Num; y?: Num; children?: Child }): Shape => ({ kind: "group", ...props, children: [children] });

/** The file of a sample field at `path`, and a button that opens a file to put there. */
export const SampleChooser = (props: { path: string; label?: string }): Node => ({ type: "sample", ...props });

/** The card that draws now, for the components below, which read its record and fields. */
function drawing() {
  const drawing = host.drawing;
  if (!drawing) {
    throw new Error("a component of a card is drawn outside a card");
  }
  return drawing;
}

const KNOB = { track: "#292c34", value: "#a9b1ff", face: "#1b1d23", pointer: "#e9ebef", label: "#7c808c", text: "#e9ebef" } as const;
/** The arc of a knob runs from -135 to +135 degrees, in turns. */
const SWEEP = 0.75;

/**
 * A knob. With `path`, it turns that field of the record, such as `rate`: a drag is smooth and
 * one undo step. With `live`, it plays a live control instead, which nothing saves. A double
 * click puts it at `default`.
 */
export function Knob(props: { path?: string; live?: string; label: string; min: number; max: number; default: number; unit?: Unit }): Node {
  const { path, live, label, min, max, unit } = props;
  const value = path !== undefined ? now.field(path) : now.control(live ?? "");
  const fraction = path !== undefined ? now.field(path, { fraction: true }) : now.control(live ?? "", { fraction: true });
  const turns: DragTurns = {
    ...(path !== undefined ? { field: path } : { control: live ?? "" }),
    min,
    max,
    unit,
    default: props.default,
    label: `Change ${label.toLowerCase()}`,
  };
  const angle = fraction.times(SWEEP).minus(SWEEP / 2);
  const radians = angle.times(2 * Math.PI);
  const [centreX, centreY] = [28, 20];
  return Space({
    key: `knob-${path ?? live}`,
    width: 56,
    height: 72,
    cursor: "ns-resize",
    drag: { y: turns },
    children: [
      Arc({ x: centreX, y: centreY, radius: 16, from: -SWEEP / 2, to: SWEEP / 2, color: KNOB.track, width: 2.5 }),
      Arc({ x: centreX, y: centreY, radius: 16, from: -SWEEP / 2, to: angle, color: KNOB.value, width: 2.5 }),
      Circle({ x: centreX, y: centreY, radius: 10.5, color: KNOB.face }),
      Line({
        from: [radians.sin().times(3).plus(centreX), radians.cos().times(-3).plus(centreY)],
        to: [radians.sin().times(9).plus(centreX), radians.cos().times(-9).plus(centreY)],
        color: KNOB.pointer,
        width: 2,
      }),
      Label({ x: centreX, y: 40, text: label, color: KNOB.label, size: 11, align: "center" }),
      Label({ x: centreX, y: 55, text: value, unit, color: KNOB.text, size: 11, align: "center" }),
    ],
  });
}

const STEP = { size: 14, gap: 3, on: "#a9b1ff", off: "#292c34", light: "#e9ebef" } as const;

/**
 * A row of steps on a pattern of the record: a click turns a step on (to `max`, the pattern's
 * max when left out) or off (to its min). `playing` names a watch whose value is the step that
 * plays, which lights up.
 */
export function Steps(props: { path: string; max?: number; playing?: string }): Node {
  const { state, fields, update } = drawing();
  const field = fields[props.path];
  const [off, on] = field?.kind === "pattern" ? [field.min, props.max ?? field.max] : [0, props.max ?? 1];
  const list = (state[props.path] as number[] | undefined) ?? [];
  const pitch = STEP.size + STEP.gap;
  return Space({
    key: `steps-${props.path}`,
    width: Math.max(list.length * pitch - STEP.gap, 0),
    height: STEP.size,
    children: list.map((value, step) => {
      const isOn = value > off;
      return Rect({
        key: String(step),
        x: step * pitch,
        y: 0,
        width: STEP.size,
        height: STEP.size,
        radius: 3,
        color: isOn ? STEP.on : STEP.off,
        border: STEP.light,
        borderWidth: props.playing === undefined ? 0 : now.watch(props.playing).floor().eq(step).times(2),
        cursor: "pointer",
        onPress: () =>
          update(isOn ? "Turn a step off" : "Turn a step on", ({ state: next }) => {
            const changed = [...list];
            changed[step] = isOn ? off : on;
            next[props.path] = changed;
          }),
      });
    }),
  });
}

/** A bar that shows a watch from 0 to 1, such as a level. */
export function Meter(props: { watch: string; label?: string }): Node {
  const level = now.watch(props.watch).clamp(0, 1).times(64);
  const bar = Space({
    width: 8,
    height: 64,
    children: [
      Rect({ x: 0, y: 0, width: 8, height: 64, radius: 2, color: "#292c34" }),
      Rect({ x: 0, y: level.negate().plus(64), width: 8, height: level, radius: 2, color: "#7ee0a0" }),
    ],
  });
  return h("div", { style: { align: "center", gap: 4 } }, bar, props.label && h("div", { style: { color: KNOB.label, fontSize: 11 } }, props.label));
}

/** A square to play with the pointer: across sets the live control `x`, up sets `y`. */
export function Pad(props: { x: string; y: string; size?: number }): Node {
  const size = props.size ?? 120;
  return Space({
    key: `pad-${props.x}-${props.y}`,
    width: size,
    height: size,
    background: "#292c34",
    radius: 6,
    cursor: "crosshair",
    drag: { x: { control: props.x, absolute: true }, y: { control: props.y, absolute: true } },
    children: [
      Circle({
        x: now.control(props.x, { fraction: true }).times(size),
        y: now.control(props.y, { fraction: true }).negate().plus(1).times(size),
        radius: 5,
        color: "#a9b1ff",
      }),
    ],
  });
}

/**
 * A surface to draw on and play: `shapes` are drawn in order. `onPress` and `onDrag` hear the
 * pointer, at `x` across and `y` down, from 0 to 1. A `Space` with a unit of a point.
 */
export function Canvas(props: {
  width: number;
  height: number;
  shapes: Shape[];
  background?: Color;
  onPress?: (x: number, y: number) => void;
  onDrag?: (x: number, y: number) => void;
}): Node {
  const { width, height, onPress, onDrag } = props;
  const across = (x: number, y: number): [number, number] => [Math.min(Math.max(x / width, 0), 1), Math.min(Math.max(y / height, 0), 1)];
  return Space({
    width,
    height,
    background: props.background ?? "#1b1d23",
    radius: 6,
    onPress: onPress && ((press) => onPress(...across(press.x, press.y))),
    drag: onDrag && { onMove: (drag) => onDrag(...across(drag.x, drag.y)) },
    children: props.shapes,
  });
}

interface DivProps {
  key?: string;
  style?: Style;
  onClick?: (press: Press) => void;
  children?: Child;
}

type Component<Props> = (props: Props) => Node;

/** Makes the nodes of JSX. `tsconfig.json` next to this file points JSX here. */
export function h<Props>(
  type: "div" | Component<Props>,
  props: (Props & DivProps) | null,
  ...children: Child[]
): Node {
  if (typeof type === "function") {
    // A component such as `Knob` takes no children: an empty list would be one more field.
    return type((children.length > 0 ? { ...props, children } : { ...props }) as Props);
  }
  return { type, key: props?.key, style: props?.style, onClick: props?.onClick, children };
}

export namespace h {
  export namespace JSX {
    export type Element = Node;
    export interface IntrinsicElements {
      div: DivProps;
    }
  }
}

/** `tone_hz` is `Tone hz`. */
export function words(name: string): string {
  const spaced = name.replaceAll("_", " ");
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

const BUTTON: Style = { paddingX: 8, paddingY: 4, radius: 6, background: "#2a2f3a" };
const CHOSEN: Style = { ...BUTTON, background: "#7c3aed", color: "#ffffff" };

/**
 * The card of a tool that has none: a knob per knob and per live control, a row of steps per
 * pattern, a button per option, toggle and trigger.
 */
export function defaultCard(fields: Fields, controls: Controls = {}) {
  return ({ state, update, fire }: Context<Record<string, unknown>>): Node => {
    const knobs: Node[] = [];
    const rows: Node[] = [];
    // Toggles and triggers share one row, so a card fits its 144 points.
    const buttons: Node[] = [];
    for (const [name, field] of Object.entries(fields)) {
      const label = field.label ?? words(name);
      if (field.kind === "knob") {
        knobs.push(
          Knob({ path: name, label, min: field.min, max: field.max, default: field.default, unit: field.unit }),
        );
      } else if (field.kind === "pattern") {
        rows.push(Steps({ path: name, max: field.max }));
      } else if (field.kind === "sample") {
        rows.push(SampleChooser({ path: name, label }));
      } else if (field.kind === "toggle") {
        const on = state[name] as boolean;
        buttons.push(
          h("div", {
            style: on ? CHOSEN : BUTTON,
            onClick: () => update(`Turn ${label.toLowerCase()} ${on ? "off" : "on"}`, ({ state: next }) => {
              next[name] = !on;
            }),
          }, label),
        );
      } else if (field.kind === "choice") {
        const chosen = state[name];
        rows.push(
          h("div", { style: { direction: "row", gap: 4, align: "center" } },
            h("div", { style: { color: "#9ca3af" } }, label),
            field.options.map((option) =>
              h("div", {
                style: option === chosen ? CHOSEN : BUTTON,
                onClick: () => update(`Set ${label.toLowerCase()} to ${option}`, ({ state: next }) => {
                  next[name] = option;
                }),
              }, String(option)),
            ),
          ),
        );
      }
    }
    for (const [name, control] of Object.entries(controls)) {
      const label = control.label ?? words(name);
      if (control.kind === "live") {
        knobs.push(Knob({ live: name, label, min: control.min, max: control.max, default: control.default }));
      } else {
        buttons.push(h("div", { style: BUTTON, onClick: () => fire(name) }, label));
      }
    }
    if (buttons.length > 0) {
      rows.push(h("div", { style: { direction: "row", gap: 4 } }, buttons));
    }
    // The rest goes beside the knobs, not under them: a card is 144 points tall.
    return h(
      "div",
      { style: { direction: "row", gap: 16 } },
      h("div", { style: { direction: "row", gap: 8 } }, knobs),
      h("div", { style: { gap: 8 } }, rows),
    );
  };
}

/** What the host reads. Not for tools. */
export const host = {
  tools: new Map<string, ToolSpec<Fields, Controls, unknown>>(),
  /** The call stack of each tool's `tool`, which tells the file that made it. */
  stacks: new Map<string, string>(),
  /** The card that draws now: its record, its fields and its `update`. */
  drawing: undefined as
    | { state: Record<string, unknown>; fields: Fields; update: Context<Record<string, unknown>>["update"] }
    | undefined,
};
