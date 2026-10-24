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

type BinSource = "magnitude" | "phase" | "hz" | "index";

/** Where a table reads: a list or file field by name, or a `buffer` by its number. */
export type TableRef = { list: string } | { buffer: number };

/**
 * The bins of a `spectral` as the runtime gets them: a graph of their own, which reads each
 * signal of the sound around it that it uses as one of `captures`.
 */
export interface Bins<R = number> {
  nodes: GraphNode[];
  feedbacks: number[];
  captures: R[];
  output: { magnitude: number; phase: number };
}

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
  | { op: "lowpass" | "highpass" | "bandpass" | "notch" | "allpass"; x: R; hz: R; q: R }
  | { op: "peak" | "lowshelf" | "highshelf"; x: R; hz: R; db: R; q: R }
  | { op: "smooth"; x: R; ms: R }
  | { op: "adsr"; gate: R; attack: R; decay: R; sustain: R; release: R }
  | { op: "hold"; x: R; when: R }
  | { op: "rise" | "change"; x: R }
  | { op: "at"; table: TableRef; index: R }
  | { op: "lookup"; table: TableRef; phase: R }
  | { op: "length"; table: TableRef }
  | { op: "spectral"; x: R; size: number; bins: Bins<R> }
  | { op: BinSource };

/** A node the runtime gets: each signal it reads is the index of a node before it. */
export type GraphNode =
  | SignalNode<number>
  | { op: "constant"; value: number }
  | { op: "write"; buffer: number; index: number; value: number }
  | { op: "capture"; index: number };

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
  /** 0 in the sound, else the `spectral` whose bins made it. */
  readonly scope: number = scope;
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
  declare readonly node: Extract<SignalNode<Operand>, { op: Source | BinSource | "param" | "feedback" }>;
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
    return new Signal({ op: "at", table: readable(this.ref), index });
  }
  /** How many values it holds. */
  get length(): Signal {
    return new Signal({ op: "length", table: readable(this.ref) });
  }
}

/** `ref`, where a table may be read: a bin of a `spectral` reads no `buffer`. */
function readable(ref: TableRef): TableRef {
  if (scope !== 0 && "buffer" in ref) throw new Error("a buffer is not read inside spectral");
  return ref;
}

/** Memory to write and read, such as a loop or a grain cloud: all 0 at first. */
export class Buffer extends Table {
  constructor(private readonly slot: number) {
    super({ buffer: slot });
  }
  /** Writes `value` at `index`, every sample. */
  write(index: Operand, value: Operand): void {
    outside("write").writes.push({ buffer: this.slot, index, value });
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
/** See `Signal.scope`, and how many `spectral` there were. */
let scope = 0;
let scopes = 0;

function building(): Building {
  if (!current) throw new Error("feedback, buffer, write, output and spectral belong inside `sound`");
  return current;
}

/** What `sound` declares, where `what` is not inside the bins of a `spectral`. */
function outside(what: string): Building {
  const declared = building();
  if (scope !== 0) throw new Error(`${what} is not used inside spectral`);
  return declared;
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
  (op: "lowpass" | "highpass" | "bandpass" | "notch" | "allpass") =>
  (x: Operand, hz: Operand, q: Operand = Math.SQRT1_2): Signal =>
    new Signal({ op, x, hz, q });
/** Filters. `q` is 0.707 when left out: no peak; higher rings at `hz`, up to 20. */
export const lowpass = filter("lowpass");
export const highpass = filter("highpass");
export const bandpass = filter("bandpass");
/** Takes out a band around `hz`, all of it at `hz`. Higher `q` is narrower. */
export const notch = filter("notch");
/** Keeps every level and shifts the phase around `hz`, for a phaser. */
export const allpass = filter("allpass");
const gained =
  (op: "peak" | "lowshelf" | "highshelf") =>
  (x: Operand, hz: Operand, db: Operand, q: Operand = Math.SQRT1_2): Signal =>
    new Signal({ op, x, hz, db, q });
/** Raises or lowers a band around `hz` by `db`, -24 to 24. Higher `q` is narrower. */
export const peak = gained("peak");
/** Raises or lowers what is below `hz` by `db`, -24 to 24, half of it at `hz`; `q` up to 1.5. */
export const lowshelf = gained("lowshelf");
/** Raises or lowers what is above `hz` by `db`, -24 to 24, half of it at `hz`; `q` up to 1.5. */
export const highshelf = gained("highshelf");
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
  const declared = outside("buffer");
  const made = new Buffer(declared.buffers.length);
  declared.buffers.push(seconds);
  return made;
}

/** One bin of a spectrum, in `spectral`. */
export interface Bin {
  /** How loud it is: 1 for a sine of 1 at its frequency. */
  mag: Signal;
  /** Where its wave is, in radians from -PI to PI. */
  phase: Signal;
  /** The frequency at its middle. */
  hz: Signal;
  /** Its number, from 0 at 0 Hz to `size / 2` at half the sample rate. */
  index: Signal;
}

/**
 * `x` through its spectrum: `each` runs once per bin of the last `size` samples, a power of
 * two from 256 to 8192, a quarter of `size` apart, and gives the bin as it goes out. Each bin
 * keeps its own memory from one spectrum to the next, so `feedback`, `hold` and `smooth` work
 * per bin; a signal from outside is read as it is at each spectrum. The sound comes out `size`
 * samples late. Inside `sound` only, in a tool of one voice.
 */
export function spectral(
  x: Operand,
  size: number,
  each: (bin: Bin) => { mag: Operand; phase: Operand },
): Signal {
  const around = outside("spectral");
  const own = ++scopes;
  current = { feedbacks: [], buffers: [], writes: [], numbers: [], bangs: [], notes: [] };
  scope = own;
  let bins: Bins<Operand>;
  try {
    const inside = current;
    const { mag, phase } = each({
      mag: new Named({ op: "magnitude" }),
      phase: new Named({ op: "phase" }),
      hz: new Named({ op: "hz" }),
      index: new Named({ op: "index" }),
    });
    const captures: Signal[] = [];
    const captured = new Map<Signal, number>();
    const { nodes, prepare, use } = layout(own, (outer) => {
      let index = captured.get(outer);
      if (index === undefined) {
        index = captures.push(outer) - 1;
        captured.set(outer, index);
      }
      return { op: "capture", index };
    });
    prepare(mag);
    prepare(phase);
    const feedbacks = inside.feedbacks.map((feedback) => use(set(feedback)));
    bins = { nodes, feedbacks, captures, output: { magnitude: use(mag), phase: use(phase) } };
  } finally {
    current = around;
    scope = 0;
  }
  return new Signal({ op: "spectral", x, size, bins });
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
  const declared = outside("output");
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
 * meanwhile.
 */
export function graphToJson(build: () => Operand | Stereo): Graph {
  current = { feedbacks: [], buffers: [], writes: [], numbers: [], bangs: [], notes: [] };
  try {
    const declared = current;
    const output = build();
    const { nodes, prepare, use } = layout(0);
    const add = (node: GraphNode) => nodes.push(node) - 1;
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
    const feedbacks = declared.feedbacks.map((feedback) => use(set(feedback)));
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

/**
 * The nodes of the signals of `own` scope, in order: a signal is one node, after the nodes it
 * reads; a number or a `Named` is a node of its own wherever it is read. A signal of the sound
 * around the bins of a `spectral` is a node `capture` makes.
 */
function layout(own: number, capture?: (outer: Signal) => GraphNode) {
  const nodes: GraphNode[] = [];
  const made = new Map<Signal, number>();
  const add = (node: GraphNode) => nodes.push(node) - 1;
  /** Adds the nodes of `operand` that are made once, the signals it reads first. */
  const prepare = (operand: Operand) => {
    if (typeof operand === "number" || made.has(operand)) return;
    if (operand.scope !== own) {
      if (capture) return;
      throw new Error("a signal of the bins of a spectral is used outside it");
    }
    if (operand instanceof Named) return;
    const node = operand.node;
    if (node.op === "spectral") {
      prepare(node.x);
      node.bins.captures.forEach(prepare);
      const bins = { ...node.bins, captures: node.bins.captures.map(use) };
      made.set(operand, add({ op: "spectral", x: use(node.x), size: node.size, bins }));
      return;
    }
    const operands = Object.entries(node).filter((entry): entry is [string, Operand] =>
      isOperand(entry[1]),
    );
    for (const [, each] of operands) prepare(each);
    const read = Object.fromEntries(operands.map(([key, each]) => [key, use(each)]));
    made.set(operand, add({ ...node, ...read } as GraphNode));
  };
  /** The node of `operand` where it is used, made now when it is a number, a `Named` or a capture. */
  const use = (operand: Operand): number => {
    prepare(operand);
    if (typeof operand === "number") return add({ op: "constant", value: quantize(operand) });
    if (operand.scope !== own && capture) return add(capture(operand));
    return operand instanceof Named ? add(operand.node) : (made.get(operand) as number);
  };
  return { nodes, prepare, use };
}

/** What `feedback` is set to. */
function set(feedback: Feedback): Operand {
  if (feedback.value === undefined) {
    throw new Error("a feedback is never set: call .set(value) on it");
  }
  return feedback.value;
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
