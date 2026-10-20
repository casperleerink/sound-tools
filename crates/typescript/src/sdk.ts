// The SDK of the project's own tools. Sound Tools writes this file when the project opens, so
// an edit here is lost. Read `agent-docs/extensions.md` first.
//
// A tool is a `.ts` or `.tsx` file next to this one that calls `tool`: the fields of its
// record, its inputs, its doc for agents, its sound and its view.

// ---------------------------------------------------------------------------------------------
// Fields of a record: saved, undoable, and what an agent writes. Whatever reads a field follows
// it: a sound that reads a number glides to it, a behaviour or a view that reads it runs again.

/** What a readout shows after the number. */
export type Unit = "hz" | "ms" | "db" | "percent";

/**
 * A number from `min` to `max`. A sound that reads it hears it as a signal that glides, and a
 * lane or a cable can move it unless it is `integer`.
 */
export interface NumberField {
  kind: "number";
  min: number;
  max: number;
  /** `min` when left out. */
  default?: number;
  /** Only whole numbers. */
  integer?: boolean;
  /** `"hz"` turns on a log scale; `"percent"` still holds 0 to 1. */
  unit?: Unit;
  /** What a view calls it. The field name, in words, when left out. */
  label?: string;
}

/** On or off. A sound hears it as a signal of 1 or 0. */
export interface BooleanField {
  kind: "boolean";
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

export interface TextField {
  kind: "text";
  /** `""` when left out. */
  default?: string;
  label?: string;
}

/**
 * A sound file: the name of a file under `assets/audio/`, or "" for none. A sound hears it as a
 * `Table` at the rate of the engine, mixed to one channel, up to 60 seconds.
 */
export interface FileField {
  kind: "file";
  label?: string;
}

/** The fields of each item of a list. */
export interface ObjectItem<F extends Fields = Fields> {
  kind: "object";
  fields: F;
}

/** Items of one kind: a field, or an object of fields. A sound hears a list of numbers as a `Table`. */
export interface ListField<I extends Field | ObjectItem = Field | ObjectItem> {
  kind: "list";
  item: I;
  /** The most items it holds. */
  max?: number;
  /** `[]` when left out. `list` types it from `item`. */
  default?: unknown[];
  label?: string;
}

export type Field = NumberField | BooleanField | ChoiceField | TextField | FileField | ListField;
export type Fields = Record<string, Field>;

/** A note of a `notes()` list. */
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

/** A whole number keeps `integer: true` in its type, so no lane or cable is typed to move it. */
export function number(field: Omit<NumberField, "kind"> & { integer: true }): NumberField & { integer: true };
export function number(field: Omit<NumberField, "kind">): NumberField;
export function number(field: Omit<NumberField, "kind">): NumberField {
  return { kind: "number", ...field };
}
export const boolean = (field: Omit<BooleanField, "kind">): BooleanField => ({ kind: "boolean", ...field });
/** `default` is one of `options`, which are at least one. */
export const choice = <const Option extends string | number>(field: {
  options: readonly [Option, ...Option[]];
  default: NoInfer<Option>;
  label?: string;
}): ChoiceField<Option> => ({ kind: "choice", ...field });
export const text = (field: Omit<TextField, "kind"> = {}): TextField => ({ kind: "text", ...field });
export const file = (field: Omit<FileField, "kind"> = {}): FileField => ({ kind: "file", ...field });

/**
 * The item of a list: a field as it is, an object of fields as an `ObjectItem`. Not `I & Fields`,
 * whose index signature would let a misspelled field of an item through.
 */
type ItemOf<I extends Field | Fields> = I extends Field ? I : I extends Fields ? ObjectItem<I> : never;

/**
 * A list whose items are `item`: a field, such as `number(...)`, or an object of fields. A field
 * an item of `default` leaves out is at its own default.
 */
export function list<const I extends Field | Fields>(
  item: I,
  options: { max?: number; label?: string; default?: ValueOf<ListField<ItemOf<I>>, false> } = {},
): ListField<ItemOf<I>> {
  const field = typeof item.kind === "string" ? item : { kind: "object", fields: item };
  return { kind: "list", item: field as ItemOf<I>, ...options };
}

/** A row of `length` numbers, such as the steps of a sequence: a list that starts as `length` times `default`. */
export function pattern(field: { length: number; min: number; max: number; default: number; label?: string }): ListField<NumberField> {
  const { length, min, max, label } = field;
  if (!Number.isInteger(length) || length < 1 || length > 1024) {
    throw new RangeError(`a pattern holds 1 to 1024 values, not ${length}`);
  }
  return list(number({ min, max, default: field.default }), { max: length, label, default: Array<number>(length).fill(field.default) });
}

/** The fields of a `Note`, with their ranges. */
export const noteFields = {
  start: number({ min: 0, max: 2 ** 31, integer: true }),
  length: number({ min: 1, max: 2 ** 31, default: 1, integer: true }),
  pitch: number({ min: 0, max: 127, default: 60, integer: true }),
  velocity: number({ min: 1, max: 127, default: 100, integer: true }),
} as const;

/** Notes in ticks, 960 to a quarter note, from where the parent's code plays them: a list of `noteFields`. */
export function notes(field: { default?: Note[]; label?: string } = {}): ListField<ObjectItem<typeof noteFields>> {
  return list(noteFields, field);
}

// ---------------------------------------------------------------------------------------------
// Inputs: what plays a tool and nothing saves, as a performer plays an instrument. A cable, a
// view or code plays them with `send`. Every sound also has the input `notes`, which plays its voices.

/** A bang: a signal that is 1 for the one sample after it is sent. */
export interface BangInput {
  kind: "bang";
  label?: string;
}

/** A number input is a `number(...)` that nothing saves: it starts at its default, and glides. */
export type Input = NumberField | BangInput;
export type Inputs = Record<string, Input>;

export const bang = (input: Omit<BangInput, "kind"> = {}): BangInput => ({ kind: "bang", ...input });

/** The names of the inputs in `I` of kind `K`. */
type InputNames<I extends Inputs, K extends Input["kind"]> = {
  [Name in keyof I]: I[Name] extends infer Each ? (Each extends { kind: K } ? Name : never) : never;
}[keyof I] &
  string;

/** A value of a field as a record holds it; `Filled` says whether the fields of an item may be left out. */
type ValueOf<F, Filled extends boolean> = F extends NumberField
  ? number
  : F extends BooleanField
    ? boolean
    : F extends ChoiceField<infer Option>
      ? Option
      : F extends FileField | TextField
        ? string
        : F extends ListField<infer I>
          ? Array<I extends ObjectItem<infer G> ? (Filled extends true ? ValuesOf<G> : StateOf<G>) : ValueOf<I, Filled>>
          : never;

/** The `state` of a record as a file may hold it: a field left out is at its default. */
export type StateOf<S extends Fields> = { -readonly [Name in keyof S]?: ValueOf<S[Name], false> };

/**
 * The `state` of a record as a behaviour, a view and the control loop get it: every field, at
 * its default where the record leaves it out. Not readonly as the fields of a tool are, so
 * `update` can change it.
 */
export type ValuesOf<S extends Fields> = { -readonly [Name in keyof S]: ValueOf<S[Name], true> };

/** What a sound hears of a field: a number or a boolean as a `Param`, a choice as its value, a file or a list of numbers as a `Table`. */
type Heard<F> = F extends NumberField | BooleanField
  ? Param
  : F extends ChoiceField<infer Option>
    ? Option
    : F extends FileField | ListField<NumberField>
      ? Table
      : never;

/** What `sound` gets: the fields it can hear, and every input, a bang as a `Param` that is 1 for one sample. */
export type SoundOf<S extends Fields, I extends Inputs> = {
  [Name in keyof S as [Heard<S[Name]>] extends [never] ? never : Name]: Heard<S[Name]>;
} & { [Name in keyof I]: Param };

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

/** Where a table reads: a list or file field by name, or a `buffer` by its number. */
export type TableRef = { list: string } | { buffer: number };

/** A node of the sound graph, with `R` for each signal it reads. */
type SignalNode<R> =
  | { op: Source }
  /** A field or an input. */
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
  | { op: "length"; table: TableRef };

/** A node the runtime gets: each signal it reads is the index of a node before it. */
export type GraphNode =
  | SignalNode<number>
  | { op: "constant"; value: number }
  | { op: "write"; buffer: number; index: number; value: number };

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

/** A value read where it is used, with no memory: a source, a field, an input, a feedback. */
class Named extends Signal {
  declare readonly node: Extract<SignalNode<Operand>, { op: Source | "param" | "feedback" }>;
}

/**
 * A field or an input of the tool. A `Signal`, so the sound uses it as any other; it is no
 * number, so code cannot branch on it: a turn of a knob must not run `sound` again.
 */
export class Param extends Named {
  constructor(readonly name: string) {
    super({ op: "param", name });
  }
}

/** A list to read by index or by phase: a list of numbers or a file of the record, or a `buffer`. */
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
  numbers: Array<{ name: string; value: Operand }>;
  bangs: Array<{ name: string; value: Operand }>;
  notes: Array<{ name: string; gate: Operand; pitch: Operand; velocity: Operand }>;
}

let current: Building | undefined;

function building(): Building {
  if (!current) throw new Error("feedback, buffer, write and output belong inside `sound`");
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

/** Notes from signals: one sounds while `gate` is above 0, at the MIDI `pitch` it starts with. */
export interface Notes {
  gate: Operand;
  pitch: Operand;
  /** 0 to 1, as it starts; 0.8 when left out. */
  velocity?: Operand;
  bang?: never;
}

/**
 * An output of the sound, which a parent connects to another child: a number, in the units of
 * the number it drives, which a view also reads as `now.value(name)` and `outputs[name]`;
 * `{ bang }`, a bang where it goes above 0; or `Notes`, a note that ends where the gate falls or
 * the pitch moves. Inside `sound` only; a sound with more than one voice sends only numbers.
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
  current = { feedbacks: [], buffers: [], writes: [], numbers: [], bangs: [], notes: [] };
  try {
    const declared = current;
    const output = build();
    const nodes: GraphNode[] = [];
    const made = new Map<Signal, number>();
    const add = (node: GraphNode) => nodes.push(node) - 1;
    /** Adds the nodes of `operand` that are made once, the signals it reads first. */
    const prepare = (operand: Operand) => {
      if (typeof operand === "number" || operand instanceof Named || made.has(operand)) return;
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
    const numbers = declared.numbers.map(({ name, value }) => ({ name, node: use(value) }));
    // A view reads a number output as the window reads a watch.
    const watches = numbers;
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

export interface ToolSpec<S extends Fields, I extends Inputs, M, K extends ChildTools = ChildTools> {
  /** The `tool` of its records and the name of its doc: lowercase letters, digits, `-`, `_`. */
  name: string;
  /** What its view and the picker call it. */
  title: string;
  /** One line for the map of docs: when an agent should read the doc. */
  when: string;
  /** What it does and how its fields change the sound, in Markdown. Goes into its doc. */
  doc: string;
  /**
   * How many voices its sound plays at once. 1, when left out: one voice that runs all the time
   * and follows the newest note, such as an effect, a drone or a generative part. More, up to 8:
   * a voice for each note, which ends when the note has ended and the voice is silent.
   */
  voices?: number;
  /** The fields of its record. Field names are lowercase letters, digits and `_`. */
  state: S;
  /** What plays it and nothing saves: `number(...)` and `bang()`. Names are as field names. */
  inputs?: I;
  /**
   * Its sound: a `Signal` made with the functions of the sound graph, or a `Stereo` pair. A sound
   * that reads `input` has an audio input. `input` when left out, which passes the sound through.
   */
  sound?: (fields: SoundOf<S, I>) => Operand | Stereo;
  /**
   * Makes it a parent: its record is `state/<name>/instance.json` and the records next to it
   * are its children. A pure function of its record and its children that says how they and
   * its own ports connect. It runs in Bun when what it read changes, and what it declares plays a
   * moment later; the sound plays on meanwhile. Its own `sound` hears what it connects to its
   * own `{ port: "audio" }` as `input`, and is its `audio` output.
   */
  behaviour?: (parent: Parent<S, K>) => Declaration<K>;
  /**
   * The children it has, by name, and the tool of each: `{ bass: "timeline", lead: drone }`,
   * where `drone` is what `tool` returned for a tool of this project. TypeScript then checks
   * every connection, `send` and edit of them against their tools, and a child of another tool,
   * a missing one or one it does not name is a problem. Leave it out for children whose names a
   * record decides.
   */
  children?: K & LiteralTools<K>;
  /**
   * What the control loop and the view of an instance keep, and nothing saves: positions of
   * a simulation, the last thing played. Made new for each instance, and when the code loads.
   */
  memory?: () => M;
  /**
   * The control loop: about 30 times a second while the window is open, for each instance. It
   * plays the sound with `send`, as a performer would, and keeps what it needs in `memory`.
   */
  tick?: (tool: Tick<ValuesOf<S>, I, M, K>) => void;
  /**
   * Its view, drawn in the room the window gives it: a card in a rack, or the whole window for
   * an instance at the top of a project. Without one it gets a knob per number and a button per
   * option, boolean and bang.
   */
  view?: (view: View<ValuesOf<S>, I, M, K>) => Child;
  /**
   * A key of the computer keyboard goes down or up: while its view shows at the top or after a
   * click on its card, those `keys` names, else every plain key; and wherever the focus is, those
   * `globalKeys` names.
   */
  onKey?: (tool: Context<ValuesOf<S>, I, M, K>, key: Key) => void;
  /**
   * The keys `onKey` takes while its view has the keys, by name: `"delete"`, `"a"`, `"left"`, or
   * with cmd, ctrl or alt held first, `"cmd-c"`. The rest go on to `globalKeys`. Left out, every
   * plain key.
   */
  keys?: readonly KeyName[];
  /**
   * The keys `onKey` hears wherever the focus is, outside a text field, while its view is the
   * whole window: `["space"]` to play and stop. The app keeps undo, opening, quitting and its
   * panel; nothing else plays a key by itself.
   */
  globalKeys?: readonly Exclude<KeyName, AppKey>[];
  /**
   * Hears what reaches its `notes`, whatever plays them: the keyboard, a clip, a cable, a
   * parent's `play` and `send`; not its own. Its sound plays them by itself as well.
   */
  onNotes?: (tool: Context<ValuesOf<S>, I, M, K>, event: HeardEvent) => void;
  /**
   * Keeps what one of its `recorder` children took, with `update`: such as a new child
   * `{ name: take.name, tool: take.tool, state: take.state }`. Its files are written already.
   * With no `onTake` a take keeps only its files.
   */
  onTake?: (tool: Context<ValuesOf<S>, I, M, K>, take: Take) => void;
}

/**
 * What a `recorder` took, as the record of a child that keeps it: notes as a `timeline`, sound as
 * a `region`. `recorder` is the child that took it, `name` a name of its files.
 */
export type Take = { recorder: string; name: string } & (
  | { tool: "timeline"; state: StateOfTool<"timeline"> }
  | { tool: "region"; state: StateOfTool<"region"> }
);

/** A record and the records of its children: what a behaviour gets, and what `update` changes. */
export interface Part<State, K extends ChildTools = ChildTools> {
  state: State;
  /** The records next to its own, in name order, each with its own children. */
  get children(): DraftChildren<K>;
  /** A new list: push a new child as `{ name, tool, state }`, its own children left out or given. */
  set children(children: Array<OwnedChild<K, true> | NewChild<K>>);
}

/** What a behaviour gets. */
export type Parent<S extends Fields, K extends ChildTools = ChildTools> = Frozen<{ state: ValuesOf<S>; children: OwnedChild<K>[] }>;

/** The name of a tool: a built-in one, which `Tools` describes, or one of this project. */
export type ToolName = keyof Tools | (string & {});

declare const entry: unique symbol;

/**
 * A tool of this project as `tool` returns it: its name, which is what the records hold, typed
 * with an entry as `Tools` has one. Named in a parent's `children`, its `state` and inputs are
 * checked; a name typed as a string checks nothing.
 */
export type ProjectTool<Name extends string = string, Entry = unknown> = Name & { readonly [entry]: Entry };

declare const slot: unique symbol;

/**
 * A child of any tool that takes `port`, in a parent's `children`: `{ synth: takes("notes") }` is
 * a synth, a sampler, a plugin or an instrument of the project, as `offers("notes")` lists them.
 * Its state is any; its ports are those of what fills it.
 */
export type Takes<Port extends "notes" | "audio" = "notes" | "audio"> = `takes:${Port}` & { readonly [slot]: Port };

/** A slot of `children` for any tool that takes `port`, see `Takes`. */
export function takes<Port extends "notes" | "audio">(port: Port): Takes<Port> {
  return `takes:${port}` as Takes<Port>;
}

declare const filled: unique symbol;
/**
 * The tool of a child a `Takes` slot holds: one that takes `Port`, whichever it is. Not a string
 * to code, so a check such as `child.tool === "timeline"` tells the other children apart; its
 * name is `String(child.tool)`.
 */
export type TakingTool<Port extends string = string> = { readonly [filled]: Port };

/** The tool a child of `T` has, as code reads it: a `TakingTool` for a `Takes`. */
type ToolOf<T> = T extends Takes<infer Port> ? TakingTool<Port> : T;
/** The tool a new child of `T` may be: any for a `Takes`. */
type NewToolOf<T> = T extends Takes ? ToolName : T;

/** The entry of `T`: in `Tools`, or of a `ProjectTool`. */
type EntryOf<T> = T extends keyof Tools ? Tools[T] : T extends ProjectTool<string, infer Entry> ? Entry : {};

/**
 * The tools of the children of a parent by their names, as its `children` names them. A name that
 * ends in `*` stands for every other name that starts as it does: `{ "*": track }` is any number of
 * tracks, named by their records, and `{ "ret-*": bus, "*": track }` buses whose names start with
 * `ret-` and tracks of every other name.
 */
export type ChildTools = Record<string, ToolName>;

/**
 * The children a tool of `T` names: those of a tool of this project; none of a built-in one;
 * any of a tool named by a string.
 */
type ChildrenOfTool<T> = string extends T ? ChildTools : EntryOf<T> extends { children: infer K extends ChildTools } ? K : {};

/** The `state` of a record of `T` as the file holds it; any value for a tool named by a string. */
export type StateOfTool<T> = EntryOf<T> extends { state: infer State } ? State : unknown;

/** The `state` of a record of `T` as `update` gives it: every field that has a default at it where the file leaves it out. */
export type FilledStateOfTool<T> =
  EntryOf<T> extends { filled: infer Filled }
    ? Filled
    : EntryOf<T> extends { state: infer State; defaults: infer Defaults extends string }
      ? Omit<State, Defaults> & { [Field in Defaults & keyof State]-?: Exclude<State[Field], undefined> }
      : StateOfTool<T>;

/** `children` as written. A tool name widened to `string` would check nothing, so it is refused. */
type LiteralTools<K extends ChildTools> = {
  [Name in keyof K]: string extends K[Name] ? "the tool names were widened to string: write the children as `const children = { ... } as const` and give the behaviour the return type `Declaration<typeof children>`, or leave the return type out" : K[Name];
};

/** A name as a child has it: any that starts as a pattern such as `"ret-*"` does. */
type ChildNamed<Name extends string> = Name extends `${infer Prefix}*` ? `${Prefix}${string}` : Name;

/** The names of its children. */
type ChildName<K extends ChildTools> = ChildNamed<keyof K & string>;

/** A child of a parent: its record. */
export type OwnedChild<K extends ChildTools = ChildTools, Edit extends boolean = false> = {
  [Name in keyof K & string]: {
    /**
     * Its file name without `.json`: lowercase letters, digits, `-` and `_`. It is the id of the
     * record, which never changes: keep a title in a field.
     */
    readonly name: ChildNamed<Name>;
    /** Its `tool`, such as `"instrument.synth"`. */
    tool: ToolOf<K[Name]>;
    /** Its `state`, with every field that has a default at it, as `state` of the parent. */
    state: FilledStateOfTool<K[Name]>;
  } & (Edit extends true
    ? {
        /** Its own children, to edit as these, in name order. */
        get children(): DraftChildren<ChildrenOfTool<K[Name]>>;
        /** A new list: push a new child as `{ name, tool, state }`, its own children left out or given. */
        set children(children: Array<OwnedChild<ChildrenOfTool<K[Name]>, true> | NewChild<ChildrenOfTool<K[Name]>>>);
      }
    : {
        /**
         * Its own children, in name order, when it is a parent, such as the clips of a track.
         * Typed as the `children` of its tool name them.
         */
        children: OwnedChild<ChildrenOfTool<K[Name]>>[];
      });
}[keyof K & string];

/** The children of a record in an `update`: edit them, `push` a new one, or set a new list. */
export interface DraftChildren<K extends ChildTools> extends Array<OwnedChild<K, true>> {
  push(...children: Array<OwnedChild<K, true> | NewChild<K>>): number;
}

/** A child an `update` makes: its record, and its own children if it has some. */
export type NewChild<K extends ChildTools = ChildTools> = {
  [Name in keyof K & string]: {
    readonly name: ChildNamed<Name>;
    tool: NewToolOf<K[Name]>;
    state: StateOfTool<K[Name]>;
    children?: NewChild<ChildrenOfTool<K[Name]>>[];
  };
}[keyof K & string];

/** The keys that name a port, one for each kind of what it carries. */
type PortKey = "port" | "notes" | "number" | "bang";

/** A port named under the key `K` and no other, so a connection joins two of one kind. */
type Keyed<K extends PortKey, Name extends string = string> = Record<K, Name> & { [Other in Exclude<PortKey, K>]?: never };
/**
 * The names a child of tool `T` has under the key `P`, as its entry says: any where it says
 * none. Any output (`Out`) of a tool of this project too, as its `sound` declares them.
 */
type PortName<T, P extends PortKey | "input", Out extends boolean> = [Out, T] extends [true, ProjectTool]
  ? string
  : [Out, P] extends [true, "number"]
    ? EntryOf<T> extends { out: infer Sent extends string } ? Sent : string
    : P extends keyof EntryOf<T>
      ? EntryOf<T>[P]
      : string;
/**
 * A port of a child under the key `P`, of a name its tool has as an input, or as an output with
 * `Out`. A port deeper down, such as `{ child: "bass/mix", number: "gain_db" }`, is a port of a
 * child of a child: its name is not checked.
 */
type PortOf<P extends PortKey, K extends ChildTools, Out extends boolean = false, Names extends PortKey | "input" = P> =
  | {
      [Name in keyof K & string]: { child: ChildNamed<Name> } & Keyed<P, PortName<K[Name], Names, Out> & string>;
    }[keyof K & string]
  | ({ child: DeepName<K> } & Keyed<P>);

/** The path of a child of one of its children, at any depth. */
type DeepName<K extends ChildTools> = `${ChildName<K>}/${string}`;
/** A port of the parent itself under the key `K`: no `child`. */
type OwnOf<K extends PortKey, Name extends string = string> = { child?: never } & Keyed<K, Name>;

/** A sound port of a child, or of a child of a child as `"bass/mix"`: `audio` when left out. A sound that reads `input` has an `audio` input; every sound an `audio` output. */
export type ChildPort<K extends ChildTools = ChildTools> = Partial<Keyed<"port">> & { child: ChildName<K> | DeepName<K> };
/** Notes of a child: its input `notes`, which plays its voices, or with `Out` notes it sends out. */
export type ChildNotes<K extends ChildTools = ChildTools, Out extends boolean = false> = PortOf<"notes", K, Out>;
/**
 * A number of a child: one it takes, such as the field `cutoff_hz` of a `filter`, a number field
 * or a number input of a tool of the project, or with `Out` one it sends out with `output`.
 */
export type ChildNumber<K extends ChildTools = ChildTools, Out extends boolean = false> = PortOf<"number", K, Out>;
/** A bang of a child: a bang input it takes, or with `Out` one it sends out with `output`. */
export type ChildBang<K extends ChildTools = ChildTools, Out extends boolean = false> = PortOf<"bang", K, Out>;

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
  | { from: ChildNumber<K, true>; to: ChildNumber<K> | OwnOf<"number"> }
  | { from: ChildBang<K, true>; to: ChildBang<K> | OwnOf<"bang"> };

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
  key: PlainKey;
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

/** The names of the numbers in `S` a lane or a cable moves: those that are not whole numbers. */
type MovedNames<S extends Fields> = { [Name in keyof S]: S[Name] extends { kind: "number"; integer: true } ? never : S[Name] extends NumberField ? Name : never }[keyof S] & string;

/** The entry of a tool of the project with fields `S`, inputs `I` and children `K`. */
type ProjectEntry<S extends Fields, I extends Inputs, K extends ChildTools> = {
  state: StateOf<S>;
  filled: ValuesOf<S>;
  /** What a number cable moves: a number field or a number input. */
  number: MovedNames<S> | InputNames<I, "number">;
  /** What `send` plays with a number. */
  input: InputNames<I, "number">;
  bang: InputNames<I, "bang">;
  notes: "notes";
  children: K;
};

/** Makes a tool of the project. Export what it returns for a parent to name in its `children`. */
export function tool<
  const Name extends string,
  const S extends Fields,
  const I extends Inputs = {},
  M = {},
  const K extends ChildTools = ChildTools,
>(spec: ToolSpec<S, I, M, K> & { name: Name }): ProjectTool<Name, ProjectEntry<S, I, K>> {
  if (host.tools.has(spec.name)) {
    throw new Error(`a tool named ${spec.name} is already defined`);
  }
  host.tools.set(spec.name, spec as unknown as ToolSpec<Fields, Inputs, unknown>);
  host.stacks.set(spec.name, new Error().stack ?? "");
  return spec.name as ProjectTool<Name, ProjectEntry<S, I, K>>;
}

/** When what `send` plays happens: at `at`, a `time` of the control loop at most 10 s ahead, or at once. */
export type At = { at?: number };

/** An input `send` plays: one of the tool's own by name, its `notes`, or one of a child. */
export type SendTo<I extends Inputs, K extends ChildTools = ChildTools> =
  | InputNames<I, "number" | "bang">
  | "notes"
  | PortOf<"number", K, false, "input">
  | ChildBang<K>
  | ChildNotes<K>
  | Deeper;

/** An input of an instance deeper down, by its path such as `"voice/rec"`: its name is not checked. */
type Deeper = { child: `${string}/${string}` } & (Keyed<"number"> | Keyed<"bang"> | Keyed<"notes", "notes">);

/** What `send` takes after the input: a number for a number, a note event for notes, nothing for a bang. */
type SendArgs<P, I extends Inputs> = P extends "notes" | { notes: string }
  ? [event: NoteEvent, options?: At]
  : P extends { bang: string }
    ? [options?: At]
    : P extends { number: string }
      ? [value: number, options?: At]
      : P extends keyof I
        ? I[P] extends BangInput
          ? [options?: At]
          : [value: number, options?: At]
        : never;

/**
 * What plays an instance as a performer would: nothing saves it and no undo takes it back. What
 * has an `at` happens on that sample, or at once when it has passed. A parent plays a child the
 * same way, by its name.
 */
export interface Performer<I extends Inputs, K extends ChildTools = ChildTools> {
  /**
   * Plays an input: `send("rate", 2)` moves the number input `rate`, `send("hit")` sends the bang
   * `hit`, `send("notes", { type: "noteOn", pitch: 60, velocity: 0.8 })` plays the tool's voices,
   * and `{ child, number }`, `{ child, bang }` or `{ child, notes }` plays a child's, or one
   * deeper by its path, `"voice/rec"`. At most 256 wait for their `at`: past that a `noteOn` is
   * dropped, and anything else plays at once.
   */
  send<P extends SendTo<I, K>>(input: P, ...value: SendArgs<P, I>): void;
  /**
   * Plays a note on its voices, or on `child`'s: a `noteOn` and its `noteOff` `seconds` later
   * (0.25 when left out), or with `hold` only the `noteOn`. `velocity` is 0.8 when left out.
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

/** What all code of an instance gets: its view, `tick`, `onKey` and `onNotes`. */
export interface Context<State, I extends Inputs = Inputs, M = unknown, K extends ChildTools = ChildTools>
  extends Performer<I, K> {
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
  /**
   * Opens a file the composer chooses and copies it into `assets/audio/`: its name there, for a
   * `file` field, or `null` when they chose none.
   */
  openFile(options?: { title?: string }): Promise<string | null>;
  /**
   * Tells the window what the composer works on: the child at `path`, such as `"bass"` or
   * `"bass/verse"`, or nothing with `null`. The MIDI keyboard and the computer keys play the
   * `notes` of the nearest of it and its parents that has them; `span`, in ticks, is what Export
   * selection exports. Nothing is saved.
   */
  select(path: string | null, options?: { span?: { from: number; to: number } }): void;
  /**
   * Opens a menu of `items` after a press: under the div, button or card title pressed, as a
   * select's list, or where the pointer is after a right press or a press of a space. Answers
   * the place of the item the composer picked, or `null` when they picked none. `notes` are
   * quiet lines under the items; `width` is in points, 320 when left out.
   */
  openMenu(items: readonly MenuItem[], options?: { notes?: readonly string[]; width?: number }): Promise<number | null>;
  /** What the window offers to put in a child: devices that take `"notes"` and play them, or take `"audio"` and change it. Built-in devices, plugins and the tools of the project. With `at`, a child's path, `here` says which of them is there. */
  offers(takes: "notes" | "audio", options?: { at?: string }): Promise<Offers>;
  /** What a lane or a cable can move of the child at `path`, such as `"bass/filter"`: each number's name and range. */
  numbers(path: string): Promise<MovedNumber[]>;
  /** Puts the device of the offer `key` at the child `path`, at its defaults and in place of what is there, as one undo step `label`. */
  addOffer(key: string, path: string, label: string): void;
  /** What its code keeps and nothing saves. A binding that read a field of it runs again when code sets that field. */
  memory: Memory<M>;
  /**
   * The last value of each number `output` of its sound, and of its children's as `<child>.<name>`.
   * A binding that read one runs again as it moves.
   */
  outputs: Record<string, number>;
  /** The clock of the sound, in seconds: where it is now. Play `at` a little after it. */
  time: number;
  /** Where the piece plays, its tempo and its sections, and what plays, stops and moves it. */
  clock: Clock;
  /** The sources of live values: what a view shows moving without drawing again. */
  now: Now<State, I>;
}

/** What the control loop of an instance gets each time it runs. */
export interface Tick<State, I extends Inputs = Inputs, M = unknown, K extends ChildTools = ChildTools>
  extends Context<State, I, M, K> {
  /** Seconds since the last tick. */
  dt: number;
}


// ---------------------------------------------------------------------------------------------
// Views: nodes the code makes once, which the window keeps. What moves on them (a drag, a
// playhead, a meter, a knob turned) moves in the window, without running code: a live value.

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
  | { op: "drag"; id: string; part: keyof DragValues }
  | { op: "space"; key: string; part: keyof SpaceValues }
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

/** What a wave draws: see `now.scope`, `now.history` and `now.file`. */
export type WaveList =
  | { source: "scope"; name: string; channel?: Channel; frames?: number }
  | { source: "history"; of: Num; frames: number; key: string }
  | { source: "file"; asset: string; from?: number | null; to?: number | null }
  | { source: "take"; name: string };

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
  /** The sound a `recorder` child by its path records, as it grows, in the form of `file`. */
  take(name: string): WaveList;
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
    bpm: new Live({ op: "bpm" }),
    elapsed: new Live({ op: "elapsed" }),
    value: (path, options) => {
      // `bass/synth.cutoff_hz` is the field `cutoff_hz` of the child at `bass/synth`.
      const dot = path.lastIndexOf(".");
      const [child, name] = dot < 0 ? [undefined, path] : [path.slice(0, dot), path.slice(dot + 1)];
      return new Live({ op: "value", name, ...(child === undefined ? {} : { child }), played: options?.played ?? false, fraction: options?.fraction ?? false });
    },
    peak: (name, channel) => new Live({ op: "peak", name, channel }),
    devicePeak: (channel) => new Live({ op: "device_peak", channel }),
    scope: (name, options) => ({ source: "scope", name, ...options }),
    history: (of, frames) => ({ source: "history", of, frames, key: liveKey([of, frames]) }),
    file: (asset, options) => ({ source: "file", asset, ...options }),
    take: (name) => ({ source: "take", name }),
    drag: (id) =>
      part<keyof DragValues>((each) => ({ op: "drag", id, part: each }))(["startX", "startY", "dx", "dy", "x", "y", "active"]),
    space: (key) =>
      part<keyof SpaceValues>((each) => ({ op: "space", key, part: each }))(["left", "top", "right", "bottom", "pointX", "pointY"]),
  };
}

/** The sources of live values, for components of a view, which name fields by strings. */
export const now: Now<Record<string, number>> = sources();


/** The keys of the computer keyboard, as the window names them. */
const PLAIN_KEYS = [
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
type AppKey = `${"cmd" | "ctrl"}-${"z" | "n" | "o" | "q" | "l" | "b" | "k"}` | "tab";

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
export interface MovedNumber {
  name: string;
  min: number;
  max: number;
  /** `"log"` for a frequency or a time, `"fader"` for a volume in dB (`-inf` at the bottom). */
  scale: "linear" | "log" | "fader";
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

// ---------------------------------------------------------------------------------------------
// Reactivity. A view and a component run once and make the nodes the window keeps. A
// function given as a prop or a child is a binding: it runs again when a record field, memory,
// an output or a signal it read changes, and only what it gives the window changes there.

/** A function the window calls again when what it read changed. */
export type Accessor<T> = () => T;

/**
 * A prop as written: a value, which stays, or an `Accessor`, which runs again when what it
 * reads changes: `x={() => note.start}`. An accessor may give a live value: `x={() =>
 * picked() ? move.dx.plus(note.start) : note.start}`.
 */
export type Bind<T> = T | Accessor<T>;

/** Memory as code gets it: change it by setting a field. A `Set` or a `Map` is replaced, never changed in place. */
export type Memory<M> = M extends Set<infer T>
  ? ReadonlySet<T>
  : M extends Map<infer K, infer V>
    ? ReadonlyMap<K, V>
    : M extends (...args: never[]) => unknown
      ? M
      : M extends object
        ? { -readonly [Key in keyof M]: Memory<M[Key]> }
        : M;

interface Owner {
  owner: Owner | null;
  owned: Computation[];
  cleanups: Array<() => void>;
  /** The card whose nodes it makes. */
  card: CardRoot | null;
  disposed: boolean;
}

interface Computation extends Owner {
  run: () => void;
  sources: Dep[];
  /** Made earlier runs first, so a binding runs after what made it. */
  order: number;
  queued: boolean;
}

interface Dep {
  observers: Set<Computation>;
}

let listener: Computation | null = null;
let owner: Owner | null = null;
/** Above 0 while the body of a component runs, which reads nothing that changes. */
let settingUp = 0;
let batching = 0;
let queue: Computation[] = [];
let order = 0;

const READ_IN_SETUP =
  "a view and a component run once, so a value that changes is read inside a function the window calls again: " +
  "x={() => note.start}, not x={note.start}; {() => `${state.count} notes`} as a child; <For each={() => state.notes}>";

function dep(): Dep {
  return { observers: new Set() };
}

function track(read: Dep) {
  if (listener) {
    if (!read.observers.has(listener)) {
      read.observers.add(listener);
      listener.sources.push(read);
    }
  } else if (settingUp > 0) {
    throw new Error(READ_IN_SETUP);
  }
}

function notify(changed: Dep | undefined) {
  if (!changed) return;
  for (const computation of changed.observers) {
    if (!computation.queued) {
      computation.queued = true;
      queue.push(computation);
    }
  }
  if (batching === 0) flush();
}

/** Runs `change` and then, once, every binding what it changed reaches. */
export function batch<T>(change: () => T): T {
  batching += 1;
  try {
    return change();
  } finally {
    batching -= 1;
    if (batching === 0) flush();
  }
}

function flush() {
  if (queue.length === 0) return;
  batching += 1;
  try {
    while (queue.length > 0) {
      const due = queue.sort((a, b) => a.order - b.order);
      queue = [];
      for (const computation of due) {
        computation.queued = false;
        if (!computation.disposed) rerun(computation);
      }
    }
  } finally {
    batching -= 1;
  }
  // Bindings ran again: their ops go out, also when code changed memory after an `await`.
  host.changed();
}

function clean(node: Owner) {
  for (const child of node.owned) dispose(child);
  node.owned = [];
  for (const cleanup of node.cleanups.splice(0)) cleanup();
  if ("sources" in node) {
    const computation = node as Computation;
    for (const read of computation.sources) read.observers.delete(computation);
    computation.sources = [];
  }
}

function dispose(node: Owner) {
  clean(node);
  node.disposed = true;
}

function rerun(computation: Computation) {
  clean(computation);
  const [lastListener, lastOwner, lastSetup] = [listener, owner, settingUp];
  [listener, owner, settingUp] = [computation, computation, 0];
  try {
    computation.run();
  } catch (error) {
    failedIn(computation, error);
  } finally {
    [listener, owner, settingUp] = [lastListener, lastOwner, lastSetup];
  }
}

function failedIn(where: Owner, error: unknown) {
  let card: CardRoot | null = null;
  for (let at: Owner | null = where; at && !card; at = at.owner) card = at.card;
  if (!card) throw error;
  card.error ??= String(error);
}

function computation(run: () => void): Computation {
  const made: Computation = { run, sources: [], owner, owned: [], cleanups: [], card: owner?.card ?? null, disposed: false, order: order++, queued: false };
  owner?.owned.push(made);
  return made;
}

/** Runs `run` now and again whenever what it read changes. */
function effect(run: () => void) {
  rerun(computation(run));
}

/** A root of its own, disposed by hand: a row of a list, a card. */
function root<T>(make: (dispose: () => void) => T, card: CardRoot | null = owner?.card ?? null): T {
  const made: Owner = { owner, owned: [], cleanups: [], card, disposed: false };
  const [lastListener, lastOwner] = [listener, owner];
  [listener, owner] = [null, made];
  try {
    return make(() => dispose(made));
  } finally {
    [listener, owner] = [lastListener, lastOwner];
  }
}

/** Runs `cleanup` when what runs now runs again or goes. */
function onCleanup(cleanup: () => void) {
  owner?.cleanups.push(cleanup);
}

/** Reads once, without what runs now following it: what a component takes as it first draws, such as where an editor opens. */
export function untrack<T>(read: () => T): T {
  const [lastListener, lastSetup] = [listener, settingUp];
  [listener, settingUp] = [null, 0];
  try {
    return read();
  } finally {
    [listener, settingUp] = [lastListener, lastSetup];
  }
}

/** Runs the body of a component: once, reading nothing that changes. */
function setup<T>(body: () => T): T {
  const [lastListener] = [listener];
  listener = null;
  settingUp += 1;
  try {
    return body();
  } finally {
    settingUp -= 1;
    listener = lastListener;
  }
}

/** A value of the card's own that bindings follow: `const [open, setOpen] = signal(false)`. */
export function signal<T>(initial: T): [Accessor<T>, (next: T) => void] {
  let value = initial;
  const read = dep();
  return [
    () => {
      track(read);
      return value;
    },
    (next) => {
      if (!Object.is(value, next)) {
        value = next;
        notify(read);
      }
    },
  ];
}

/** A value worked out from others once per change, for many bindings to read. */
export function memo<T>(work: () => T): Accessor<T> {
  const [read, write] = signal<T>(undefined as T);
  effect(() => write(work()));
  return read;
}

/**
 * Whether a key is in a set that changes, such as the selected notes: a binding that asks for
 * one key runs again only when that key comes or goes, not when any does.
 */
export function selector<K>(keys: Accessor<ReadonlySet<K>>): (key: K) => boolean {
  const watched = new Map<K, Dep>();
  let current: ReadonlySet<K> = new Set();
  effect(() => {
    const next = keys();
    const last = current;
    current = next;
    for (const key of last) if (!next.has(key)) notify(watched.get(key));
    for (const key of next) if (!last.has(key)) notify(watched.get(key));
  });
  return (key) => {
    let read = watched.get(key);
    if (!read) {
      read = dep();
      watched.set(key, read);
    }
    track(read);
    return current.has(key);
  };
}

// Stores: the record, memory, outputs and the transport, read field by field.

const RAW = Symbol("raw");
const ALL_KEYS = Symbol("keys");
const proxies = new WeakMap<object, object>();
const fieldSources = new WeakMap<object, Map<PropertyKey, Dep>>();

/** A proxy hears an index of a list as text: `"3"`, never `3`. */
function keyOf(key: PropertyKey): PropertyKey {
  return typeof key === "number" ? String(key) : key;
}

function fieldSource(target: object, key: PropertyKey): Dep {
  key = keyOf(key);
  let sources = fieldSources.get(target);
  if (!sources) {
    sources = new Map();
    fieldSources.set(target, sources);
  }
  let found = sources.get(key);
  if (!found) {
    found = dep();
    sources.set(key, found);
  }
  return found;
}

function changed(target: object, key: PropertyKey) {
  notify(fieldSources.get(target)?.get(keyOf(key)));
}

function isWrapped(value: unknown): value is object {
  if (value === null || typeof value !== "object") return false;
  const prototype = Object.getPrototypeOf(value);
  return Array.isArray(value) || prototype === Object.prototype || prototype === null;
}

function raw<T>(value: T): T {
  return (value !== null && typeof value === "object" && (value as Record<symbol, unknown>)[RAW]) as T || value;
}

/** A store over `target`: reads are tracked; writes go through only when `writable`. */
function store<T extends object>(target: T, writable: boolean): T {
  // Already a store, such as a note of the record kept in memory.
  if ((target as Record<symbol, unknown>)[RAW]) return target;
  const known = proxies.get(target);
  if (known) return known as T;
  const proxy = new Proxy(target, {
    get(object, key, receiver) {
      if (key === RAW) return object;
      const value = Reflect.get(object, key, receiver);
      if (typeof key === "symbol" || (typeof value === "function" && !Object.hasOwn(object, key))) return value;
      // The name and the tool of a child never change: a rename is another record. So a
      // component may read them as it is made, as the key of a row.
      if ((key === "name" || key === "tool") && Object.hasOwn(object, "state")) return value;
      track(fieldSource(object, key));
      return isWrapped(value) ? store(value, writable) : value;
    },
    has(object, key) {
      if (typeof key !== "symbol") track(fieldSource(object, key));
      return Reflect.has(object, key);
    },
    ownKeys(object) {
      track(fieldSource(object, ALL_KEYS));
      return Reflect.ownKeys(object);
    },
    set(object, key, value) {
      if (!writable) {
        throw new TypeError(`${String(key)} is read only here: change the record with update(label, ({ state }) => ...)`);
      }
      const fresh = !Object.hasOwn(object, key);
      const next = raw(value);
      const last = (object as Record<PropertyKey, unknown>)[key];
      const lengthBefore = Array.isArray(object) ? object.length : 0;
      Reflect.set(object, key, next);
      if (!Object.is(last, next)) changed(object, key);
      if (Array.isArray(object) && object.length !== lengthBefore) changed(object, "length");
      if (fresh || (Array.isArray(object) && !Object.is(last, next))) changed(object, ALL_KEYS);
      return true;
    },
    deleteProperty(object, key) {
      if (!writable) throw new TypeError(`${String(key)} is read only here`);
      const had = Object.hasOwn(object, key);
      Reflect.deleteProperty(object, key);
      if (had) {
        changed(object, key);
        changed(object, ALL_KEYS);
      }
      return true;
    },
  });
  proxies.set(target, proxy);
  return proxy;
}

function sameShallow(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (!isWrapped(a) || !isWrapped(b) || Array.isArray(a) !== Array.isArray(b)) return false;
  const [left, right] = [a as Record<string, unknown>, b as Record<string, unknown>];
  const keys = Object.keys(left);
  return keys.length === Object.keys(right).length && keys.every((key) => sameShallow(left[key], right[key]));
}

/** The value each part of a store was last made like: one given again is the same, and is not walked. */
const madeFrom = new WeakMap<object, object>();

/**
 * Makes `target` hold what `next` holds, keeping each object that stays, so only bindings of
 * what changed run. A list of the same length keeps its items by place, so a moved note is the
 * same note, unless a child there has another name; one that grew or shrank keeps the items
 * that did not change by what they hold, so a note deleted before others leaves them be.
 */
function reconcile(target: Record<PropertyKey, unknown>, next: Record<PropertyKey, unknown>) {
  // Frozen values are kept as they are, so a record that did not change, and all it holds, is
  // the same object as last time: the clips of the tracks an edit left alone.
  if (madeFrom.get(target) === next) return;
  if (Object.isFrozen(next)) madeFrom.set(target, next);
  if (Array.isArray(target) && Array.isArray(next)) {
    reconcileList(target, next);
    return;
  }
  let keysChanged = false;
  for (const key of Object.keys(target)) {
    if (!Object.hasOwn(next, key)) {
      delete target[key];
      changed(target, key);
      keysChanged = true;
    }
  }
  for (const [key, value] of Object.entries(next)) {
    keysChanged ||= !Object.hasOwn(target, key);
    reconcileField(target, key, value);
  }
  if (keysChanged) changed(target, ALL_KEYS);
}

function reconcileField(target: Record<PropertyKey, unknown>, key: PropertyKey, value: unknown) {
  const last = target[key];
  if (isWrapped(last) && isWrapped(value) && Array.isArray(last) === Array.isArray(value)) {
    reconcile(last as Record<PropertyKey, unknown>, value as Record<PropertyKey, unknown>);
  } else if (!Object.is(last, value)) {
    target[key] = adopted(value);
    changed(target, key);
    if (Array.isArray(target)) changed(target, ALL_KEYS);
  }
}

/**
 * A value of `next` as a store keeps it: its own copy, which a later reconcile changes in place.
 * Each copy of a frozen value remembers it, so the next reconcile skips what did not change
 * instead of walking all of it once.
 */
function adopted<T>(value: T): T {
  if (!isWrapped(value)) return value;
  const copy: Record<PropertyKey, unknown> = Array.isArray(value) ? ([] as never) : {};
  for (const [key, each] of Object.entries(value)) copy[key] = adopted(each);
  if (Object.isFrozen(value)) madeFrom.set(copy, value);
  return copy as T;
}

/** What an item of a list is called: a child by its name and tool. Other items have none, and a list keeps them by place. */
function idOf(value: unknown): string | undefined {
  if (!isWrapped(value) || Array.isArray(value)) return undefined;
  const { name, tool } = value as { name?: unknown; tool?: unknown };
  return typeof name === "string" ? `${name}:${String(tool)}` : undefined;
}

function reconcileList(target: unknown[], next: unknown[]) {
  // A child swapped for another of the same place is another item: its rows are made again.
  if (target.length === next.length && next.every((value, index) => idOf(value) === idOf(target[index]))) {
    next.forEach((value, index) => reconcileField(target as never, index, value));
    return;
  }
  const lengthBefore = target.length;
  let start = 0;
  while (start < target.length && start < next.length && sameShallow(target[start], next[start])) start++;
  let endTarget = target.length;
  let endNext = next.length;
  while (endTarget > start && endNext > start && sameShallow(target[endTarget - 1], next[endNext - 1])) {
    endTarget--;
    endNext--;
  }
  const byContent = new Map<string, unknown[]>();
  for (let index = start; index < endTarget; index++) {
    const key = JSON.stringify(target[index]);
    byContent.set(key, [...(byContent.get(key) ?? []), target[index]]);
  }
  const middle = next.slice(start, endNext).map((value) => {
    const same = byContent.get(JSON.stringify(value))?.shift();
    return same === undefined ? { value } : { kept: same };
  });
  const unused = [...byContent.values()].flat();
  const result = middle.map((each) => {
    if ("kept" in each) return each.kept;
    const reused = isWrapped(each.value)
      ? unused.find((old) => isWrapped(old) && Array.isArray(old) === Array.isArray(each.value) && idOf(old) === idOf(each.value))
      : undefined;
    if (reused === undefined) return adopted(each.value);
    unused.splice(unused.indexOf(reused), 1);
    reconcile(reused as Record<PropertyKey, unknown>, each.value as Record<PropertyKey, unknown>);
    return reused;
  });
  const before = target.slice();
  target.splice(start, endTarget - start, ...result);
  target.forEach((value, index) => {
    if (!Object.is(before[index], value)) changed(target, index);
  });
  for (let index = target.length; index < before.length; index++) changed(target, index);
  if (target.length !== lengthBefore) changed(target, "length");
  changed(target, ALL_KEYS);
}

/** The fields of a store that bindings read now, or `null` when one reads them all. */
function readFields(target: object): string[] | null {
  const sources = fieldSources.get(target);
  if (!sources) return [];
  if ((sources.get(ALL_KEYS)?.observers.size ?? 0) > 0) return null;
  return [...sources].filter(([key, read]) => typeof key === "string" && read.observers.size > 0).map(([key]) => key as string);
}

function isRead(target: object): boolean {
  const fields = readFields(target);
  return fields === null || fields.length > 0;
}

// ---------------------------------------------------------------------------------------------
// Nodes: what a card makes, which the window keeps by id. A change sends the window only what
// changed: a prop of a node, a node in or out.

declare const nodeBrand: unique symbol;

/** A node the window draws: what JSX makes. */
export interface UiNode {
  readonly [nodeBrand]: true;
}

/** Shows what a function gives, a list, or one of two, in place: see `For` and `Show`. */
interface Region {
  readonly mount: (into: UiNode[], family: Family) => void;
}

/** Anything a node takes as a child. A function is a binding: what it gives is shown again when what it read changes. */
export type Child = UiNode | Region | ShapeLiteral | string | number | boolean | null | undefined | (() => Child) | Child[];

type Family = "div" | "text" | "space" | "shape" | "input" | "card" | "button" | "icon" | "meter" | "toggle" | "marker";
/** What the window gets of a node: a pointer handler is `true`, a drag the number of its kept drag. */
type Wired = Record<string, unknown>;

interface Made {
  readonly [nodeBrand]: true;
  id: number;
  family: Family;
  card: CardRoot;
  props: Wired;
  /** Sent to the window: changes after this go as `set`. */
  sent: boolean;
  onPress?: (press: Press) => void;
  onDrop?: (drop: Dropped) => void;
  onCommit?: (text: string) => void;
  onPower?: () => void;
  onClose?: () => void;
  onTitle?: () => void;
  drag?: Drag;
  children: Set<Made>;
  /** The region a marker stands for, which puts what it shows before the marker. */
  region?: { parent: number | null };
}

/** What the window gets of a card at once: the ops since the last time. */
type Op = [string, ...unknown[]];

/** The nodes of one card. */
interface CardRoot {
  id: number;
  ops: Op[];
  /** Props changed since the last ops, by node: sent as one `set` each. */
  changes: Map<Made, Wired>;
  drags: Map<Drag, number>;
  error?: string;
  dispose: () => void;
}

/** The window drops a press on a node an earlier Bun made, so ids start again with each Bun. */
let nextNode = 1;
const nodes = new Map<number, Made>();

function currentCard(): CardRoot {
  const card = owner?.card;
  if (!card) throw new Error("a node is made outside a view");
  return card;
}

function makeNode(family: Family): Made {
  const made: Made = { id: nextNode++, family, card: currentCard(), props: {}, sent: false, children: new Set() } as unknown as Made;
  nodes.set(made.id, made);
  return made;
}

function send(made: Made) {
  made.card.ops.push(["create", made.id, made.family, made.props]);
  made.sent = true;
}

function setProp(made: Made, name: string, value: unknown) {
  const wired = value === undefined ? null : value;
  // An object, such as a live value, is new each time its binding runs: it goes as it is.
  if (Object.is(made.props[name] ?? null, wired)) return;
  made.props[name] = wired;
  if (!made.sent) return;
  const pending = made.card.changes.get(made) ?? {};
  pending[name] = wired;
  made.card.changes.set(made, pending);
}

function insert(parent: number, child: Made, before: Made | null) {
  child.card.ops.push(["insert", parent, child.id, before?.id ?? null]);
  if (child.region) child.region.parent = parent;
}

function forget(made: Made) {
  nodes.delete(made.id);
  made.card.changes.delete(made);
  for (const child of made.children) forget(child);
}

function remove(made: Made) {
  made.card.ops.push(["remove", made.id]);
  forget(made);
}

/** The number of `drag` on its card: a drag every note shares goes to the window once. */
function keptDrag(card: CardRoot, drag: Drag): number {
  let id = card.drags.get(drag);
  if (id === undefined) {
    id = nextNode++;
    card.drags.set(drag, id);
    card.ops.push(["drag", id, { ...drag, onEnd: drag.onEnd !== undefined }]);
  }
  return id;
}

const HANDLERS = new Set(["onPress", "onDrop", "onCommit", "onPower", "onClose", "onTitle"]);

/** A node of `family` with `props`, bound where a prop is a function, and its children in it. */
function element(family: Family, props: Record<string, unknown>, children: Child[]): UiNode {
  const made = makeNode(family);
  for (const [name, value] of Object.entries(props)) {
    if (name === "children" || value === undefined) continue;
    if (HANDLERS.has(name)) {
      (made as unknown as Record<string, unknown>)[name] = value;
      made.props[name] = true;
    } else if (name === "drag") {
      made.drag = value as Drag;
      made.props.drag = keptDrag(made.card, value as Drag);
    } else if (typeof value === "function") {
      effect(() => setProp(made, name, (value as Accessor<unknown>)()));
    } else {
      made.props[name] = value;
    }
  }
  const placed: UiNode[] = [];
  for (const child of children) materialize(child, placed, family);
  send(made);
  for (const child of placed as Made[]) {
    made.children.add(child);
    insert(made.id, child, null);
  }
  return made;
}

/** A shape as a plain object, as a child of a space. */
export type ShapeLiteral = { [Kind in Shape["kind"]]: { kind: Kind } & ShapeProps<Kind> }[Shape["kind"]];

function isNode(value: unknown): value is Made {
  return typeof value === "object" && value !== null && nodes.has((value as Made).id) && (value as Made).card !== undefined;
}

/** The nodes `child` makes, in order, as children of a node of `family`. */
function materialize(child: Child, into: UiNode[], family: Family) {
  if (child === null || child === undefined || typeof child === "boolean") return;
  if (Array.isArray(child)) {
    for (const each of child) materialize(each, into, family);
  } else if (typeof child === "function") {
    dynamic(child, into, family);
  } else if (typeof child === "string" || typeof child === "number") {
    if (family === "space" || family === "shape") throw new Error(`a Space draws shapes such as Rect and Label, not the text ${JSON.stringify(child)}`);
    const text = makeNode("text");
    text.props.text = String(child);
    send(text);
    into.push(text);
  } else if ("mount" in child && typeof child.mount === "function") {
    child.mount(into, family);
  } else if (isNode(child)) {
    if ((family === "space" || family === "shape") && child.family !== "shape" && child.family !== "marker") {
      throw new Error(`a Space draws shapes such as Rect and Label, not a ${child.family}`);
    }
    if (family !== "space" && family !== "shape" && child.family === "shape") {
      throw new Error("a shape is drawn inside a Space");
    }
    into.push(child);
  } else if ("kind" in child) {
    const { kind, ...props } = child as ShapeLiteral;
    into.push(shapeNode(kind, props, []));
  } else {
    throw new Error("a child is a node, text, a list or a function that gives one: not a node of a list that is gone");
  }
}

/** A marker the nodes of a region go before, so they keep their place among their siblings. */
function marker(): Made {
  const made = makeNode("marker");
  made.region = { parent: null };
  send(made);
  return made;
}

/** Where a region puts nodes now: in its parent before `marker`, or into the list its parent is made from. */
function place(region: Made, nodesToPlace: Made[], before: Made, into: UiNode[] | null) {
  const parent = region.region?.parent;
  for (const made of nodesToPlace) {
    if (into) into.push(made);
    else if (parent !== null && parent !== undefined) insert(parent, made, before);
    const parentNode = parent === null || parent === undefined ? undefined : nodes.get(parent);
    parentNode?.children.add(made);
  }
}

/** A child that is a function: shown again whole when what it read changes; text changes in place. */
function dynamic(show: Accessor<Child>, into: UiNode[], family: Family) {
  const end = marker();
  let shown: Made[] = [];
  let first: UiNode[] | null = into;
  effect(() => {
    const value = show();
    const parent = end.region?.parent;
    const parentNode = parent === null || parent === undefined ? undefined : nodes.get(parent);
    if ((typeof value === "string" || typeof value === "number") && shown.length === 1 && shown[0]?.family === "text") {
      setProp(shown[0], "text", String(value));
      return;
    }
    const next: UiNode[] = [];
    materialize(value, next, family);
    for (const old of shown) {
      if (!next.includes(old)) {
        parentNode?.children.delete(old);
        remove(old);
      }
    }
    shown = next as Made[];
    place(end, shown, end, first);
    first = null;
  });
  into.push(end);
}

/**
 * A list of `each`, one row per item, kept by its item: a row is made once, moves when its item
 * moves and goes when its item goes; an item that is in the list twice has two rows. Its body
 * runs once, as a component's; `index` follows its place.
 * `<For each={() => state.notes}>{(note, index) => <Rect x={() => note.start} ... />}</For>`
 */
export function For<T>(props: { each: Accessor<readonly T[]>; children: (item: T, index: Accessor<number>) => Child }): Region {
  return {
    mount(into, family) {
      const end = marker();
      type Row = { nodes: Made[]; index: (next: number) => void; dispose: () => void };
      // The rows of each item, in the order it comes in the list.
      let rows = new Map<T, Row[]>();
      let order: Row[] = [];
      let first: UiNode[] | null = into;
      onCleanup(() => {
        for (const row of order) row.dispose();
      });
      effect(() => {
        const list = props.each();
        // A store list is followed as a whole, not item by item: a list of thousands costs one read.
        const items = (list as unknown as Record<symbol, unknown>)[RAW] ? (Reflect.ownKeys(list), untrack(() => [...list])) : [...list];
        untrack(() => {
          const parent = end.region?.parent;
          const parentNode = parent === null || parent === undefined ? undefined : nodes.get(parent);
          const left = new Map([...rows].map(([item, each]) => [item, [...each]]));
          const kept = items.map((item) => left.get(item)?.shift());
          for (const row of [...left.values()].flat()) {
            for (const made of row.nodes) {
              parentNode?.children.delete(made);
              remove(made);
            }
            row.dispose();
          }
          const fresh = new Set<Row>();
          const next = new Map<T, Row[]>();
          const nextOrder = items.map((item, index) => {
            let row = kept[index];
            if (row) {
              row.index(index);
            } else {
              row = root((dispose) => {
                const [readIndex, setIndex] = signal(index);
                const made: UiNode[] = [];
                materialize(setup(() => props.children(item, readIndex)), made, family);
                return { nodes: made as Made[], index: setIndex, dispose };
              });
              fresh.add(row);
            }
            next.set(item, [...(next.get(item) ?? []), row]);
            return row;
          });
          if (first) {
            for (const row of nextOrder) place(end, row.nodes, end, first);
            first = null;
          } else {
            // The rows that stay in the order they had stay put; the others move before the row after them.
            const stayed = nextOrder.filter((row) => !fresh.has(row));
            const before = new Map(order.map((row, index) => [row, index]));
            const places = stayed.map((row) => before.get(row) ?? 0);
            const inOrder = places.every((place, index) => index === 0 || place > (places[index - 1] as number));
            const staying = new Set(inOrder ? stayed : longestRising(places).map((index) => stayed[index]));
            let anchor: Made = end;
            for (let index = nextOrder.length - 1; index >= 0; index--) {
              const row = nextOrder[index] as Row;
              if (!staying.has(row)) place(end, row.nodes, anchor, null);
              anchor = row.nodes[0] ?? anchor;
            }
          }
          rows = next;
          order = nextOrder;
        });
      });
      into.push(end);
    },
  };
}

/** The places in `values` of a longest rising run, in order. */
function longestRising(values: number[]): number[] {
  const tails: number[] = [];
  const previous: number[] = new Array(values.length).fill(-1);
  values.forEach((value, index) => {
    let low = 0;
    let high = tails.length;
    while (low < high) {
      const middle = (low + high) >> 1;
      if ((values[tails[middle] as number] as number) < value) low = middle + 1;
      else high = middle;
    }
    if (low > 0) previous[index] = tails[low - 1] as number;
    tails[low] = index;
  });
  const run: number[] = [];
  for (let at = tails.at(-1) ?? -1; at >= 0; at = previous[at] as number) run.push(at);
  return run.reverse();
}

/** Several children where one goes, as `<>…</>` writes them: a component that gives two shapes. */
export function Fragment(props: { children?: Child }): Region {
  return { mount: (into, family) => materialize(props.children, into, family) };
}

/** Shows `children` while `when` holds, else `fallback`: made again each time it shows. */
export function Show(props: { when: Accessor<unknown>; children: () => Child; fallback?: () => Child }): Region {
  return {
    mount(into, family) {
      const shown = memo(() => Boolean(props.when()));
      dynamic(() => (shown() ? setup(props.children) : props.fallback ? setup(props.fallback) : null), into, family);
    },
  };
}

// ---------------------------------------------------------------------------------------------
// What a card draws.

/** A colour as `#rrggbb` or `#rrggbbaa`. */
/** A colour of the app's theme, as the window draws its own controls. */
export type ThemeColor =
  | "gray-50" | "gray-100" | "gray-200" | "gray-300" | "gray-400" | "gray-500" | "gray-600" | "gray-700" | "gray-800" | "gray-900" | "gray-950"
  | "alpha" | "blue" | "sapphire" | "sky" | "teal" | "green" | "yellow" | "peach" | "red" | "maroon" | "mauve" | "pink" | "lavender" | "rosewater" | "flamingo";
/** `#rrggbb`, `#rrggbbaa`, a colour of the theme, or one at an opacity in percent: `"alpha/10"`, `"#a9b1ff/70"`. */
export type Color = `#${string}` | ThemeColor | `${ThemeColor}/${number}`;

/** What a shape the pointer reaches can do. */
interface Pointed {
  onPress?: (press: Press) => void;
  /** A drag, made once and given to every shape it drags. */
  drag?: Drag;
  /** Its colour while the pointer is over it. */
  hover?: Bind<Color | undefined>;
  cursor?: "pointer" | "grab" | "crosshair" | "ew-resize" | "ns-resize";
  /** 0 to 1; 1 when left out. */
  opacity?: Bind<Num>;
  /** The point a drag with `snap.grid` puts on the grid: its own `x` and `y` when left out, such as the end of a note for its resize handle. */
  anchor?: Bind<{ x?: number; y?: number }>;
  /**
   * The drags it moves with, by their `id`: while one goes on, it is `dx` across and `dy` down
   * from its place, in the window, until its code answers the end. The selected notes of a piano
   * roll: `moves={() => (picked() ? ["move"] : undefined)}`, one small change as the selection does.
   */
  moves?: Bind<readonly string[] | undefined>;
  /** The drags that make it `dx` wider and `dy` taller while they go on, as `moves`: notes being resized. */
  grows?: Bind<readonly string[] | undefined>;
}

/**
 * Something a space draws. Positions and sizes are in the units of the space; `radius`, `width`
 * of a line and `size` of text are in points.
 */
export type Shape =
  /**
   * Shapes moved together by `x` and `y`, which count from there: one live value for all of
   * them. With `clip`, a box from there out of which they do not show, nor hear the pointer: the
   * notes of a clip end at its end. `moves` and `grows` move it and its box as a shape's do.
   */
  | ({ kind: "group"; x?: Bind<Num>; y?: Bind<Num>; clip?: Bind<{ width: Num; height: Num }> } & Pick<Pointed, "moves" | "grows">)
  | (Pointed &
      (
        | { kind: "rect"; x: Bind<Num>; y: Bind<Num>; width: Bind<Num>; height: Bind<Num>; color: Bind<Paint>; radius?: Bind<number>; border?: Bind<Color>; borderWidth?: Bind<Num> }
        | { kind: "circle"; x: Bind<Num>; y: Bind<Num>; radius: Bind<Num>; color: Bind<Paint> }
        | { kind: "line"; from: Bind<readonly [Num, Num]>; to: Bind<readonly [Num, Num]>; color: Bind<Paint>; width?: Bind<number> }
        /** Along a circle from one angle to another, in turns clockwise from the top: a dial. */
        | { kind: "arc"; x: Bind<Num>; y: Bind<Num>; radius: Bind<number>; from: Bind<Num>; to: Bind<Num>; color: Bind<Paint>; width?: Bind<number> }
        /**
         * Text at `x`, `y`, its top. A live number shows as a readout in `unit`, and a tick, such as
         * `now.tick`, as `"position"` (`5.3`), `"time"` (`1:07`) or `"signature"` (the one there).
         */
        | { kind: "text"; x: Bind<Num>; y: Bind<Num>; text: Bind<string | Live>; unit?: Unit | "pan" | "position" | "time" | "signature" | "beats" | "note"; color?: Bind<Paint>; size?: number; align?: "left" | "center" | "right" }
        /**
         * The values of `list` across its box, oldest at the left, `low` at its bottom and `high` at
         * its top (-1 and 1 when left out): a line `size` points thick, or with `band` the lowest to
         * the highest under each point, as a waveform.
         */
        | { kind: "wave"; x: Bind<Num>; y: Bind<Num>; width: Bind<Num>; height: Bind<Num>; list: Bind<WaveList>; color: Bind<Paint>; low?: number; high?: number; band?: boolean; size?: number }
      ));

/** A colour, or one between two by a live value: see `blend`. */
export type Paint = Color | { mix: readonly [Color, Color]; by: Num };

/** `from` at 0, `to` at 1, and between: a light that glows with a level. */
export const blend = (from: Color, to: Color, by: Num): Paint => ({ mix: [from, to], by });

type ShapeProps<Kind extends Shape["kind"]> = Omit<Extract<Shape, { kind: Kind }>, "kind">;

function shapeNode(kind: Shape["kind"], props: object, children: Child[]): UiNode {
  return element("shape", { kind, ...props }, children);
}

const shape =
  <Kind extends Exclude<Shape["kind"], "group">>(kind: Kind) =>
  (props: ShapeProps<Kind>): UiNode =>
    shapeNode(kind, props, []);
export const Rect = shape("rect");
export const Circle = shape("circle");
export const Line = shape("line");
export const Arc = shape("arc");
export const Label = shape("text");
export const Wave = shape("wave");
/** Shapes moved together, see `Shape`. */
export const Group = ({ children, ...props }: ShapeProps<"group"> & { children?: Child }): UiNode => shapeNode("group", props, [children]);

/**
 * A surface that draws shapes in units of its own, such as ticks across and notes down. It
 * scrolls over `extent` and zooms in `zoom` in the window, with no code, and draws only what
 * shows. Without `unit`, a unit is a point.
 */
export interface SpaceProps {
  /** Names it for `now.space(key)`, and keeps its scroll and zoom. */
  key?: string;
  /** Its size in points; `grow` takes the room left in its parent. */
  width?: Bind<number>;
  height?: Bind<number>;
  grow?: boolean;
  /** Points per unit at zoom 1. */
  unit?: { x?: number; y?: number };
  /** What it scrolls over, in units; an axis without one does not scroll, and shows from 0. */
  extent?: Bind<{ x?: readonly [number, number]; y?: readonly [number, number] }>;
  /** How far it zooms in and out, as a factor; an axis without one does not zoom. Cmd and scroll, or a pinch. */
  zoom?: { x?: readonly [number, number]; y?: readonly [number, number] };
  /** Where it is scrolled to when it first draws, in units; the window keeps where it is after. */
  scroll?: { x?: number; y?: number };
  /** A range across that fills its width once it is laid out, from `before` points right of its left edge to `after` points left of its right one, as far as `zoom` goes: an editor that opens on what it shows. Once per `key`. */
  fit?: { x: readonly [number, number]; before?: number; after?: number };
  /**
   * The key of a space whose scroll and zoom it shows on an axis, and moves as it scrolls: the
   * ruler and the keys of a piano roll. Give it the same `unit`, `extent` and `zoom` on that axis.
   */
  follow?: { x?: string; y?: string };
  background?: Bind<Color>;
  radius?: number;
  /** A press where no shape that hears the pointer is. */
  onPress?: (press: Press) => void;
  /** A drag from where no shape that hears the pointer is, such as a marquee. */
  drag?: Drag;
  /** Files or a carried value let go on it, where in its units. */
  onDrop?: (drop: Dropped) => void;
  cursor?: Pointed["cursor"];
  children?: Child;
}

/** A space, see `SpaceProps`. */
export const Space = ({ children, ...props }: SpaceProps): UiNode => element("space", props, [children]);

/** A text field: it shows `value` until it is typed in, and calls `onCommit` with its text on enter or when it loses the keys. Escape shows `value` again. */
export interface InputProps {
  key?: string;
  value: Bind<string>;
  placeholder?: string;
  /** In points; 160 when left out. */
  width?: number;
  /** It takes the keys when it is made, with its text selected. */
  focus?: boolean;
  onCommit: (text: string) => void;
}

/** A text field, see `InputProps`. */
export const Input = (props: InputProps): UiNode => element("input", { ...props }, []);

/**
 * The card of an instance below this tool's, by its `path` as `now.value` names it, such as
 * `"bass/synth"`: the built-in device's own card, a plugin's with its window, or a tool of the
 * project's view. Its power icon shows `on` with `onPower`, its close icon with `onClose`.
 */
export interface DeviceCardProps {
  key?: string;
  path: Bind<string>;
  on?: Bind<boolean>;
  onPower?: () => void;
  onClose?: () => void;
  /** Its title is a menu: a chevron after it, and a press calls this, such as to put another device in its place. */
  onTitle?: () => void;
}

/** The card of an instance, see `DeviceCardProps`. */
export const DeviceCard = ({ path, ...props }: DeviceCardProps): UiNode => element("card", { instance: path, ...props }, []);

/** An icon of the app, by its name. */
export type IconName =
  | "arrow-left" | "arrow-right" | "arrow-up" | "audio-lines" | "automation" | "bot" | "check"
  | "chevron-down" | "chevron-left" | "chevron-right" | "chevron-up" | "circle" | "circle-alert"
  | "circle-check" | "claude" | "copy" | "device-analyzer" | "device-compressor" | "device-delay"
  | "device-drum-pad" | "device-eq" | "device-filter" | "device-gate" | "device-limiter"
  | "device-modulation" | "device-plugin" | "device-reverb" | "device-sampler" | "device-saturator"
  | "device-synth" | "device-utility" | "device-wavetable" | "ellipsis" | "eq-bell" | "eq-high-cut"
  | "eq-high-shelf" | "eq-low-cut" | "eq-low-shelf" | "eq-notch" | "eye" | "eye-off" | "folder" | "info"
  | "keyboard-music" | "loader-circle" | "lock" | "lock-open" | "metronome" | "mic" | "minus" | "music"
  | "panel-left" | "pause" | "pencil" | "piano" | "play" | "plus" | "power" | "redo-2" | "search" | "section"
  | "send" | "settings" | "shield" | "skip-back" | "skip-forward" | "sliders-horizontal" | "sparkles"
  | "square" | "tempo-change" | "time-signature" | "trash-2" | "triangle-alert" | "undo-2" | "volume-2"
  | "volume-x" | "x" | "zap";

/** An icon of the app, 16 points when `size` is left out, in the text's colour unless `color` says. */
export const Icon = (props: { name: Bind<IconName>; size?: number; color?: Bind<Color> }): UiNode => element("icon", { ...props }, []);

/**
 * A stereo meter as the window draws its own: bars that fall, a peak line that holds, a clip light
 * a click clears. Of the peaks `of` names as `now.peak` does, or of what the device plays. Up the
 * side unless `horizontal`; `length` along its bars, in points.
 */
export const PeakMeter = (props: { key?: string; of?: string; horizontal?: boolean; length?: number }): UiNode =>
  element("meter", { ...props }, []);

/**
 * A switch of the app: a letter (`label`), an `icon` with `label` as its tooltip and a `count`
 * after it, or a `dot`. Coloured while `on`, `color` when given; its code turns it in `onPress`.
 */
export interface ToggleProps {
  key?: string;
  label?: Bind<string>;
  icon?: Bind<IconName>;
  count?: Bind<number>;
  dot?: boolean;
  on: Bind<boolean>;
  color?: Bind<Color>;
  disabled?: Bind<boolean>;
  onPress?: (press: Press) => void;
}

/** A switch, see `ToggleProps`. */
export const Toggle = (props: ToggleProps): UiNode => element("toggle", { ...props }, []);

/** A button of the app: a `label`, an `icon` or both, as the window's own buttons look. */
export interface ButtonProps {
  key?: string;
  label?: Bind<string>;
  icon?: Bind<IconName>;
  /** `primary` when left out. `solid`, `subtle` and `ghost` take `color` as their accent. */
  variant?: Bind<"primary" | "subtle" | "ghost" | "solid">;
  color?: Bind<Color>;
  /** 24, 28 or 32 points tall; `md` when left out. */
  size?: "xs" | "sm" | "md";
  /** A pill, round at its ends. */
  rounded?: boolean;
  disabled?: Bind<boolean>;
  onPress?: (press: Press) => void;
}

/** A button, see `ButtonProps`. */
export const Button = (props: ButtonProps): UiNode => element("button", { ...props }, []);

/**
 * Shows its children in the window's title bar, in the middle of the room right of the project
 * menu, when this tool's view is the window's: the transport of a song. Anywhere else it shows
 * nothing.
 */
export const TitleBar = ({ children, style }: { children?: Child; style?: Bind<Style> }): UiNode =>
  element("div", { titleBar: true, style }, [children]);

/** How a `div` lays out and looks. Numbers are points. */
export interface Style {
  /** How children line up. A div is a column unless this says `row`. */
  direction?: "row" | "column";
  gap?: number;
  padding?: number;
  paddingX?: number;
  paddingY?: number;
  paddingLeft?: number;
  paddingRight?: number;
  paddingTop?: number;
  paddingBottom?: number;
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
  /** Its children scroll along this axis where they do not fit its size, as a long rack does. */
  scroll?: "x" | "y";
  /** `tabular`: digits of one width, so a number that changes does not move what follows. */
  font?: "tabular";
  weight?: "normal" | "medium" | "semibold";
  /** Its background while the pointer is over it. */
  hover?: Color;
  /** It never shrinks below what it needs, in a row too narrow. */
  keep?: boolean;
  /** One line, cut with an ellipsis where it does not fit. */
  truncate?: boolean;
}

interface DivProps {
  key?: string;
  style?: Bind<Style>;
  /** A click on it, or a press of the right button. */
  onPress?: (press: Press) => void;
  /** A drag of it carries this to a node that hears drops, such as another row of a list it reorders. */
  carry?: string;
  /** Files from the system, or what a dragged `div` carries, let go on it. */
  onDrop?: (drop: Dropped) => void;
  /**
   * The key of a space whose scroll its children move with, on an axis, cut off at its edges:
   * the heads of the rows of a timeline, which scroll with it. Rows as tall as the space's unit.
   */
  follow?: { x?: string; y?: string };
  children?: Child;
}

type Component<Props> = (props: Props) => UiNode | Region;

/** Makes the nodes of JSX. `tsconfig.json` next to this file points JSX here. */
export function h<Props>(type: "div" | Component<Props>, props: (Props & DivProps) | null, ...children: Child[]): UiNode {
  if (typeof type === "function") {
    // One child as it is, such as the row of a `For`; a component without children gets none.
    const given = children.length === 0 ? {} : { children: children.length === 1 ? children[0] : children };
    return setup(() => type({ ...props, ...given } as Props)) as UiNode;
  }
  return element("div", { ...props }, children);
}

export namespace h {
  export namespace JSX {
    export type Element = UiNode | Region;
    export interface IntrinsicElements {
      div: DivProps;
    }
    export interface ElementChildrenAttribute {
      children: {};
    }
  }
}

// ---------------------------------------------------------------------------------------------
// Components made of the blocks above.

/** The view that draws now, for the components below, which read its record and fields. */
function drawing() {
  const drawing = host.drawing;
  if (!drawing) {
    throw new Error("a component of a view is drawn outside a view");
  }
  return drawing;
}

/** The arc of a knob runs from -135 to +135 degrees, in turns. */
const SWEEP = 0.75;

/**
 * A knob on a number, as the app's knobs look: a field of the record such as `rate`, whose drag
 * is smooth and one undo step, or a number input, which nothing saves. `value` names it as
 * `now.value` does. With `played`, it shows the field as it plays, so it moves with a lane. A
 * double click puts it at `default`. A `bipolar` one, such as a pan, draws its arc from the top;
 * `unit: "pan"` reads `C`, `25L`, `100R`.
 */
export function Knob(props: { value: string; label: string; min: number; max: number; default: number; unit?: Unit | "pan"; played?: boolean; bipolar?: boolean }): UiNode {
  const { value: name, label, min, max, unit, played, bipolar } = props;
  const value = now.value(name, { played });
  const fraction = now.value(name, { played, fraction: true });
  const turns: DragTurns = { value: name, min, max, unit: unit === "pan" ? undefined : unit, default: props.default, label: `Change ${label.toLowerCase()}` };
  const angle = fraction.times(SWEEP).minus(SWEEP / 2);
  const radians = angle.times(2 * Math.PI);
  const [centreX, centreY, radius] = [28, 18, 16.75];
  const along = (distance: number) => [radians.sin().times(distance).plus(centreX), radians.cos().times(-distance).plus(centreY)] as const;
  return Space({
    key: `knob-${name}`,
    width: 56,
    height: 72,
    cursor: "ns-resize",
    drag: { y: turns },
    children: [
      Arc({ x: centreX, y: centreY, radius, from: -SWEEP / 2, to: SWEEP / 2, color: "alpha/10", width: 2.5 }),
      Arc({ x: centreX, y: centreY, radius, from: bipolar ? 0 : -SWEEP / 2, to: angle, color: "gray-950", width: 2.5 }),
      Circle({ x: centreX, y: centreY, radius: 10.5, color: "gray-300" }),
      Line({ from: along(3.5), to: along(8), color: "gray-950", width: 2 }),
      Label({ x: centreX, y: 38, text: label, color: "gray-800", size: 12, align: "center" }),
      Label({ x: centreX, y: 54, text: value, unit, color: "gray-950", size: 12, align: "center" }),
    ],
  });
}

const STEP = { size: 14, gap: 3, on: "#a9b1ff", off: "#292c34", light: "#e9ebef" } as const;

/**
 * A row of steps on a list of numbers of the record, such as a `pattern`: a click turns a step
 * on (to `max`, the item's max when left out) or off (to its min). `playing` names an output,
 * as `now.value` does, whose value is the step that plays, which lights up.
 */
export function Steps(props: { path: string; max?: number; playing?: string }): UiNode {
  const { state, fields, update } = drawing();
  const field = fields[props.path];
  const item = field?.kind === "list" && field.item.kind === "number" ? field.item : undefined;
  const [off, on] = [item?.min ?? 0, props.max ?? item?.max ?? 1];
  const list = () => (state[props.path] as number[] | undefined) ?? [];
  const length = untrack(() => list().length);
  const isOn = (step: number) => (list()[step] ?? off) > off;
  const pitch = STEP.size + STEP.gap;
  return Space({
    key: `steps-${props.path}`,
    width: Math.max(length * pitch - STEP.gap, 0),
    height: STEP.size,
    children: Array.from({ length }, (_, step) =>
      Rect({
        x: step * pitch,
        y: 0,
        width: STEP.size,
        height: STEP.size,
        radius: 3,
        color: () => (isOn(step) ? STEP.on : STEP.off),
        border: STEP.light,
        borderWidth: props.playing === undefined ? 0 : now.value(props.playing).floor().eq(step).times(2),
        cursor: "pointer",
        onPress: () => {
          const wasOn = isOn(step);
          update(wasOn ? "Turn a step off" : "Turn a step on", ({ state: next }) => {
            const changed = [...list()];
            changed[step] = wasOn ? off : on;
            next[props.path] = changed;
          });
        },
      }),
    ),
  });
}

/** A bar that shows a number from 0 to 1, such as a level an output sends: `value` names it as `now.value` does. */
export function Meter(props: { value: string; label?: string }): UiNode {
  const level = now.value(props.value).clamp(0, 1).times(64);
  const bar = Space({
    width: 8,
    height: 64,
    children: [
      Rect({ x: 0, y: 0, width: 8, height: 64, radius: 2, color: "#292c34" }),
      Rect({ x: 0, y: level.negate().plus(64), width: 8, height: level, radius: 2, color: "#7ee0a0" }),
    ],
  });
  return h("div", { style: { align: "center", gap: 4 } }, bar, props.label && h("div", { style: { color: "gray-800", fontSize: 11 } }, props.label));
}

/** A square to play with the pointer: across moves the number input `x`, up moves `y`. */
export function Pad(props: { x: string; y: string; size?: number }): UiNode {
  const size = props.size ?? 120;
  return Space({
    key: `pad-${props.x}-${props.y}`,
    width: size,
    height: size,
    background: "#292c34",
    radius: 6,
    cursor: "crosshair",
    drag: { x: { value: props.x, absolute: true }, y: { value: props.y, absolute: true } },
    children: [
      Circle({
        x: now.value(props.x, { fraction: true }).times(size),
        y: now.value(props.y, { fraction: true }).negate().plus(1).times(size),
        radius: 5,
        color: "#a9b1ff",
      }),
    ],
  });
}

/** Points of a drag up for one beat per minute more. */
const TEMPO_DRAG = 2;
/** The width of the position strip, and of its knob. */
const STRIP = { width: 200, knob: 8 } as const;

/** A readout of a tick of the clock as it moves, in the pill's text. */
function clockText(key: string, width: number, unit: "position" | "time" | "beats" | "note", color: Color = "gray-950", text: Live = now.tick): UiNode {
  return Space({ key, width, height: 18, children: [Label({ x: 0, y: 0, text, unit, color, size: 14 })] });
}

/** What `Transport` records with: whether it records, what a press does, and whether it can. */
export interface TransportRecord {
  on: Accessor<boolean>;
  press: () => void;
  disabled?: Accessor<boolean>;
}

/**
 * The window's transport, a pill: play and pause, stop, `record` when given, where the piece
 * plays as a bar and a beat and as a time, a strip to move through the piece up to `end` when
 * given, the tempo, which a drag up or down changes as one undo step, the time signature, the
 * computer keys, the click and the meter of what the device plays. Put it in a `TitleBar` to
 * show it where the window's own was. Everything that moves moves without code.
 */
export function Transport(props: { clock: Clock; record?: TransportRecord; end?: Accessor<number | null> }): UiNode {
  const { clock, record } = props;
  const tempo = now.drag("transport-tempo");
  const end = () => props.end?.() ?? null;
  const pill: Style = {
    direction: "row", align: "center", gap: 12, height: 36, paddingLeft: 4, paddingRight: 16,
    radius: 18, background: "#292b30", borderColor: "alpha/10", borderWidth: 1, keep: true,
  };
  return h("div", { key: "transport", style: pill },
    h("div", { style: { direction: "row", align: "center", gap: 4 } },
      Button({
        key: "play", icon: () => (clock.playing ? "pause" : "play"), variant: () => (clock.playing ? "subtle" : "ghost"),
        color: "green", size: "sm", rounded: true, onPress: () => (clock.playing ? clock.pause() : clock.play()),
      }),
      Button({ key: "stop", icon: "square", variant: "ghost", size: "sm", rounded: true, onPress: () => clock.stop() }),
      record && Button({
        key: "record", icon: "circle", variant: () => (record.on() ? "solid" : "ghost"), color: "red", size: "sm", rounded: true,
        disabled: () => record.disabled?.() ?? false, onPress: record.press,
      }),
    ),
    clockText("transport-position", 28, "position"),
    clockText("transport-time", 34, "time", "gray-700"),
    () => {
      const last = end();
      if (last === null || last <= 0) return null;
      const seconds = secondsOf(clock.tempo_map, last);
      const knob = now.tick.over(last).clamp(0, 1).times(STRIP.width - STRIP.knob);
      return [
        Space({
          key: "transport-strip", width: STRIP.width, height: 16, cursor: "pointer",
          onPress: ({ x }) => clock.jump(Math.round((Math.min(Math.max(x, 0), STRIP.width) / STRIP.width) * last)),
          children: [
            Rect({ x: 0, y: 7.5, width: STRIP.width, height: 1, color: "alpha/10" }),
            Rect({ x: 0, y: 7.5, width: knob.plus(STRIP.knob / 2), height: 1, color: "gray-950" }),
            Rect({ x: knob, y: 4, width: STRIP.knob, height: STRIP.knob, radius: STRIP.knob / 2, color: "gray-950" }),
          ],
        }),
        h("div", { style: { font: "tabular", color: "gray-700" } }, `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`),
      ];
    },
    Space({
      key: "transport-tempo",
      width: 64,
      height: 18,
      cursor: "ns-resize",
      drag: {
        id: "transport-tempo",
        axis: "y",
        onEnd: ({ dy }) => {
          const at = clock.tick;
          const bpm = Math.min(Math.max(Math.round(clock.bpm - dy / TEMPO_DRAG), 10), 1000);
          clock.update("Change the tempo", ({ tempo_map }) => {
            const change = tempo_map.tempo_changes.findLast((change) => change.tick <= at);
            if (change) change.bpm = bpm;
          });
        },
      },
      children: [
        Label({ x: 0, y: 0, text: now.bpm.minus(tempo.dy.over(TEMPO_DRAG)).round(), color: "gray-950", size: 14 }),
        Label({ x: 32, y: 2, text: "bpm", color: "gray-700", size: 12 }),
      ],
    }),
    h("div", { style: { direction: "row", align: "center" } },
      clockText("transport-beats", 14, "beats"),
      Space({ key: "transport-slash", width: 14, height: 18, children: [Label({ x: 3, y: 2, text: "/", color: "gray-700", size: 12 })] }),
      clockText("transport-note", 14, "note"),
    ),
    Button({
      key: "computer-keys", icon: "keyboard-music", variant: () => (clock.computerKeys ? "primary" : "ghost"),
      color: () => (clock.computerKeys ? undefined : "gray-700") as Color, size: "sm", rounded: true,
      onPress: () => clock.setComputerKeys(!clock.computerKeys),
    }),
    Button({
      key: "click", icon: "metronome", variant: () => (clock.click ? "primary" : "ghost"),
      color: () => (clock.click ? undefined : "gray-700") as Color, size: "sm", rounded: true,
      onPress: () => clock.setClick(!clock.click),
    }),
    PeakMeter({ key: "transport-meter", horizontal: true }),
  );
}

/** The seconds from the start to `tick` through the tempo changes of `map`. */
export function secondsOf(map: Frozen<TempoMap>, tick: number): number {
  const changes = [...map.tempo_changes].sort((a, b) => a.tick - b.tick);
  let seconds = 0;
  for (const [index, change] of changes.entries()) {
    const next = changes[index + 1]?.tick ?? Infinity;
    if (tick <= change.tick) break;
    seconds += ((Math.min(tick, next) - change.tick) / 960) * (60 / change.bpm);
  }
  return seconds;
}

/** `tone_hz` is `Tone hz`. */
export function words(name: string): string {
  const spaced = name.replaceAll("_", " ");
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

const BUTTON: Style = { paddingX: 8, paddingY: 4, radius: 6, background: "#2a2f3a" };
const CHOSEN: Style = { ...BUTTON, background: "#7c3aed", color: "#ffffff" };

/**
 * The view of a tool that has none: a knob per number and per number input, a row of steps per
 * list of numbers, a chooser per file, a button per option, boolean and bang.
 */
export function defaultView(fields: Fields, inputs: Inputs = {}) {
  return ({ state, update, send, openFile }: Context<Record<string, unknown>>): UiNode => {
    const knobs: Child[] = [];
    const rows: Child[] = [];
    // Booleans and bangs share one row, so a card fits its room.
    const buttons: Child[] = [];
    const knob = (name: string, field: NumberField) =>
      Knob({ value: name, label: field.label ?? words(name), min: field.min, max: field.max, default: field.default ?? field.min, unit: field.unit });
    for (const [name, field] of Object.entries(fields)) {
      const label = field.label ?? words(name);
      if (field.kind === "number") {
        knobs.push(knob(name, field));
      } else if (field.kind === "list" && field.item.kind === "number") {
        rows.push(Steps({ path: name }));
      } else if (field.kind === "file") {
        rows.push(
          h("div", { style: { direction: "row", gap: 8, align: "center" } },
            label,
            h("div", { style: { color: "#9ca3af" } }, () => (state[name] as string) || "No sound"),
            h("div", {
              style: BUTTON,
              onPress: async () => {
                const chosen = await openFile({ title: `Choose ${label.toLowerCase()}` });
                if (chosen !== null) update(`Choose ${label.toLowerCase()}`, ({ state: next }) => {
                  next[name] = chosen;
                });
              },
            }, "Choose…"),
          ),
        );
      } else if (field.kind === "boolean") {
        buttons.push(
          h("div", {
            style: () => (state[name] ? CHOSEN : BUTTON),
            onPress: () => {
              const on = state[name] as boolean;
              update(`Turn ${label.toLowerCase()} ${on ? "off" : "on"}`, ({ state: next }) => {
                next[name] = !on;
              });
            },
          }, label),
        );
      } else if (field.kind === "choice") {
        rows.push(
          h("div", { style: { direction: "row", gap: 4, align: "center" } },
            h("div", { style: { color: "#9ca3af" } }, label),
            field.options.map((option) =>
              h("div", {
                style: () => (option === state[name] ? CHOSEN : BUTTON),
                onPress: () => update(`Set ${label.toLowerCase()} to ${option}`, ({ state: next }) => {
                  next[name] = option;
                }),
              }, String(option)),
            ),
          ),
        );
      }
    }
    for (const [name, input] of Object.entries(inputs)) {
      if (input.kind === "number") {
        knobs.push(knob(name, input));
      } else {
        buttons.push(h("div", { style: BUTTON, onPress: () => (send as (input: string) => void)(name) }, input.label ?? words(name)));
      }
    }
    if (buttons.length > 0) {
      rows.push(h("div", { style: { direction: "row", gap: 4 } }, buttons));
    }
    // The rest goes beside the knobs, not under them: a card is no taller than its room.
    return h(
      "div",
      { style: { direction: "row", gap: 16 } },
      h("div", { style: { direction: "row", gap: 8 } }, knobs),
      h("div", { style: { gap: 8 } }, rows),
    );
  };
}

// ---------------------------------------------------------------------------------------------
// For the host.

/** What the components of a card read while it is made. */
type Drawing = { state: Record<string, unknown>; fields: Fields; update: Context<Record<string, unknown>>["update"] };

/** Makes the nodes of a card: `draw` runs once. The window gets them with `takeOps`. */
function mount(id: number, draw: () => Child, drawn: Drawing): CardRoot {
  const card: CardRoot = { id, ops: [], changes: new Map(), drags: new Map(), dispose: () => {} };
  root((dispose) => {
    let top: Made | undefined;
    card.dispose = () => {
      dispose();
      if (top) forget(top);
    };
    host.drawing = drawn;
    try {
      const made: UiNode[] = [];
      materialize(setup(draw), made, "div");
      const [only] = made as Made[];
      top = only;
      if (made.length !== 1 || !only || only.family === "marker" || only.family === "text") {
        top = makeNode("div");
        send(top);
        for (const child of made as Made[]) {
          top.children.add(child);
          insert(top.id, child, null);
        }
      }
      card.ops.push(["root", (top as Made).id]);
    } catch (error) {
      card.dispose();
      throw error;
    } finally {
      host.drawing = undefined;
    }
  }, card);
  return card;
}

/** What changed on `card` since the last time, as ops for the window, or why it failed. */
function takeOps(card: CardRoot): { ops: Op[] } | { error: string } {
  if (card.error !== undefined) return { error: card.error };
  const ops = card.ops;
  for (const [made, props] of card.changes) ops.push(["set", made.id, made.family, props]);
  card.ops = [];
  card.changes.clear();
  return { ops };
}

/** A press, the end of a drag, a drop, a field's text or a card's icon, on the node `id`. */
function nodeEvent(id: number, kind: "press" | "end" | "commit" | "drop" | "power" | "close" | "title", payload: never) {
  const made = nodes.get(id);
  if (!made) return;
  switch (kind) {
    case "commit":
      return made.onCommit?.((payload as { text: string }).text);
    case "power":
      return made.onPower?.();
    case "close":
      return made.onClose?.();
    case "title":
      return made.onTitle?.();
    default: {
      const handler = { press: made.onPress, end: made.drag?.onEnd, drop: made.onDrop }[kind];
      return handler?.(payload);
    }
  }
}

/** What the host reads. Not for tools. */
export const host = {
  /** Whether `name` names a key as `KeyName` does. */
  isKeyName: (name: string) => PLAIN_KEYS.includes(name.replace(/^(cmd-)?(ctrl-)?(alt-)?/, "") as never),
  tools: new Map<string, ToolSpec<Fields, Inputs, unknown>>(),
  /** The call stack of each tool's `tool`, which tells the file that made it. */
  stacks: new Map<string, string>(),
  /** The view that draws now: its record, its fields and its `update`. */
  drawing: undefined as Drawing | undefined,
  mount,
  takeOps,
  nodeEvent,
  store,
  reconcile,
  adopted,
  readFields,
  isRead,
  batch,
  /** Called once bindings ran again; the host sends their ops. */
  changed: () => {},
  /** Whether a binding runs now, or a component is made: what reads there is followed. */
  binding: () => listener !== null || settingUp > 0,
};
