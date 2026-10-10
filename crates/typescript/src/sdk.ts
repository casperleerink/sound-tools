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
 * are, so `sound` runs again when it changes and the new sound fades in.
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

export type Field = KnobField | ToggleField | ChoiceField | PatternField | SampleField;
export type Fields = Record<string, Field>;

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

type ValueOf<F> = F extends KnobField
  ? number
  : F extends ToggleField
    ? boolean
    : F extends ChoiceField<infer Option>
      ? Option
      : F extends PatternField
        ? number[]
        : F extends SampleField
          ? string
          : never;

/**
 * The `state` of a record of a tool with these fields. A field left out is at its default.
 * Not readonly as the fields of a tool are, so `update` can change it.
 */
export type StateOf<S extends Fields> = { -readonly [Name in keyof S]?: ValueOf<S[Name]> };

/**
 * What `sound` gets: a choice as its value, a pattern as a `Table`, anything else as a `Param`,
 * which is a `Signal`.
 */
export type SoundOf<S extends Fields, C extends Controls> = {
  [Name in keyof S]: S[Name] extends ChoiceField<infer Option>
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
}

let current: Building | undefined;

function building(): Building {
  if (!current) throw new Error("feedback, buffer, write and watch belong inside `sound`");
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

/** Shows `value` to the card as `watches[name]`. Inside `sound` only. */
export function watch(name: string, value: Operand): void {
  building().watches.push({ name, value });
}

/**
 * For the host: the graph of a sound. `build` makes the output; what it declares is kept
 * meanwhile. A signal is one node, after the nodes it reads; a number or a `Named` is a node
 * of its own wherever it is read.
 */
export function graphToJson(build: () => Operand | Stereo): Graph {
  current = { feedbacks: [], buffers: [], writes: [], watches: [] };
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
    const watches = declared.watches.map(({ name, value }) => ({ name, node: use(value) }));
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

export interface ToolSpec<S extends Fields, C extends Controls, M> {
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
  /** Its sound: a `Signal` made with the functions of the sound graph, or a `Stereo` pair. */
  sound: (fields: SoundOf<S, C>) => Operand | Stereo;
  /**
   * What the control loop and the card of an instance keep, and nothing saves: positions of
   * a simulation, the last thing played. Made new for each instance, and when the code loads.
   */
  memory?: () => M;
  /**
   * The control loop: about 30 times a second while the window is open, for each instance. It
   * plays the sound with `set` and `fire`, as a performer would, and keeps what it needs in
   * `memory`. It does not change the record: that is the composer's.
   */
  tick?: (tool: Tick<StateOf<S>, C, M>) => void;
  /** Its card. Without one it gets a knob per knob and a button per option and toggle. */
  card?: (card: Card<StateOf<S>, C, M>) => Node;
  /** Its page: the whole window, for an instance at the top of a project with no arrangement. */
  page?: (page: Page<StateOf<S>, C, M>) => Node;
  /**
   * A key of the computer keyboard goes down or up, while its page shows or after a click on
   * its card. Plain keys are then the tool's: space and `r` do not play or record.
   */
  onKey?: (tool: Handler<StateOf<S>, C, M>, key: Key) => void;
  /**
   * The MIDI keyboard plays, while the tool is on the track it plays or at the top of the
   * project. An instrument plays the notes by itself as well.
   */
  onMidi?: (tool: Handler<StateOf<S>, C, M>, message: Midi) => void;
}

/** A key of the computer keyboard: its name, such as `"a"`, `"1"`, `"space"` or `"left"`. */
export interface Key {
  key: string;
  /** Pressed; `false` when it comes up. */
  down: boolean;
}

/**
 * A message of the MIDI keyboard. `pitch` is MIDI, 69 is A4; a velocity or a value is 0 to 1;
 * the bend is -1 to 1. The sustain pedal is controller 64, the mod wheel 1.
 */
export type Midi =
  | { type: "noteOn"; pitch: number; velocity: number }
  | { type: "noteOff"; pitch: number }
  | { type: "cc"; controller: number; value: number }
  | { type: "bend"; value: number };

/** Makes a tool of the project. */
export function tool<const S extends Fields, const C extends Controls = {}, M = {}>(
  spec: ToolSpec<S, C, M>,
): void {
  if (host.tools.has(spec.name)) {
    throw new Error(`a tool named ${spec.name} is already defined`);
  }
  host.tools.set(spec.name, spec as unknown as ToolSpec<Fields, Controls, unknown>);
}

/**
 * What plays an instance as a performer would: nothing saves it and no undo takes it back.
 * `at` is a `time` of the control loop: what has one happens on that sample, or at once when
 * it has passed.
 */
export interface Performer<C extends Controls> {
  /** Moves a live control. */
  set(control: ControlNames<C, "live">, value: number): void;
  /** Fires a trigger. */
  fire(control: ControlNames<C, "trigger">, options?: { at?: number }): void;
  /**
   * Plays a note on the tool's own voices, as a key held for `seconds` (0.25 when left out),
   * or with `hold` until `release`: `pitch` is MIDI, 69 is A4, and `velocity` is 0 to 1 (0.8
   * when left out). An instrument plays each note on a voice of its own; a source follows the
   * newest.
   */
  play(
    pitch: number,
    options?: { seconds?: number; hold?: boolean; velocity?: number; at?: number },
  ): void;
  /** Lets go of a note `play` holds. */
  release(pitch: number, options?: { at?: number }): void;
}

/** What the control loop, the cards and the handlers of an instance get. */
interface Instance<State, C extends Controls, M> extends Performer<C> {
  /** The record as it is now. */
  state: State;
  /** What the control loop and the cards keep. A click may change it too; the card draws again after. */
  memory: M;
  /** The last value of each `watch` of the sound, by name. A card draws again as they move. */
  watches: Record<string, number>;
}

/** What the control loop of an instance gets each time it runs. */
export interface Tick<State, C extends Controls, M> extends Instance<State, C, M> {
  /** Seconds since the last tick. */
  dt: number;
  /** The clock of the sound, in seconds: where it is now. Play `at` a little after it. */
  time: number;
}

/** What `onKey` and `onMidi` get: what a card gets, and the `time` of a tick. */
export interface Handler<State, C extends Controls, M>
  extends Card<State, C, M>,
    Pick<Tick<State, C, M>, "time"> {}

// ---------------------------------------------------------------------------------------------
// Cards

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

/** What a card or a page gets each time it draws. */
export interface Card<State, C extends Controls = Controls, M = unknown> extends Instance<State, C, M> {
  /**
   * Changes the record as one undo step called `label`. `change` gets a copy to edit. A record
   * the tool does not accept is refused, and the window says why.
   */
  update(label: string, change: (state: State) => void): void;
}

/** What a page gets: what a card gets, and the room it has. */
export interface Page<State, C extends Controls = Controls, M = unknown> extends Card<State, C, M> {
  /** Its width and height in points. It draws again when the window changes. */
  size: { width: number; height: number };
}

/** A shape of a `Canvas`, in points from its top left. */
export type Shape =
  | { kind: "circle"; x: number; y: number; radius: number; color: Color }
  | { kind: "rect"; x: number; y: number; width: number; height: number; color: Color; radius?: number }
  | { kind: "line"; from: [number, number]; to: [number, number]; color: Color; width?: number };

/** The props of each element of a card but `div`, by its type. */
interface Elements {
  knob: { path?: string; live?: string; label: string; min: number; max: number; default: number; unit?: Unit };
  steps: { path: string; max?: number; playing?: string };
  sample: { path: string; label?: string };
  meter: { watch: string; label?: string };
  pad: { x: string; y: string; size?: number };
  canvas: {
    width: number;
    height: number;
    shapes: Shape[];
    background?: Color;
    onPress?: (x: number, y: number) => void;
    onDrag?: (x: number, y: number) => void;
  };
}

export type Child = Node | string | number | boolean | null | undefined | Child[];

export type Node =
  | { type: "div"; style?: Style; onClick?: () => void; children: Child[] }
  | { [Type in keyof Elements]: { type: Type } & Elements[Type] }[keyof Elements];

const element =
  <Type extends keyof Elements>(type: Type) =>
  (props: Elements[Type]) => ({ type, ...props });

/**
 * A knob. With `path`, it turns that field of the record, such as `rate`: a drag is smooth and
 * one undo step. With `live`, it plays a live control instead, which nothing saves. A double
 * click puts it at `default`.
 */
export const Knob = element("knob");
/**
 * A row of steps on a pattern of the record: a click turns a step on (to `max`, the pattern's
 * max when left out) or off (to its min). `playing` names a watch whose value is the step that
 * plays, which lights up.
 */
export const Steps = element("steps");
/** The file of a sample field at `path`, and a button that opens a file to put there. */
export const SampleChooser = element("sample");
/** A bar that shows a watch from 0 to 1, such as a level. */
export const Meter = element("meter");
/**
 * A surface to draw on and play: `shapes` are drawn in order. `onPress` and `onDrag` hear the
 * pointer, at `x` across and `y` down, from 0 to 1.
 */
export const Canvas = element("canvas");
/** A square to play with the pointer: across sets the live control `x`, up sets `y`. */
export const Pad = element("pad");

interface DivProps {
  style?: Style;
  onClick?: () => void;
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
  return { type, style: props?.style, onClick: props?.onClick, children };
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
  return ({ state, update, fire }: Card<Record<string, unknown>, Controls, unknown>): Node => {
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
        const on = (state[name] as boolean | undefined) ?? field.default;
        buttons.push(
          h("div", {
            style: on ? CHOSEN : BUTTON,
            onClick: () => update(`Turn ${label.toLowerCase()} ${on ? "off" : "on"}`, (next) => {
              next[name] = !on;
            }),
          }, label),
        );
      } else {
        const chosen = (state[name] as string | number | undefined) ?? field.default;
        rows.push(
          h("div", { style: { direction: "row", gap: 4, align: "center" } },
            h("div", { style: { color: "#9ca3af" } }, label),
            field.options.map((option) =>
              h("div", {
                style: option === chosen ? CHOSEN : BUTTON,
                onClick: () => update(`Set ${label.toLowerCase()} to ${option}`, (next) => {
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
};
