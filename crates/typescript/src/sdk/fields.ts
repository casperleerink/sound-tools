// Fields of a record: saved, undoable, and what an agent writes. Whatever reads a field follows
// it: a sound that reads a number glides to it, a behaviour or a view that reads it runs again.

import { Param, Sample, Table } from "./sound";

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
 * `Sample` at the rate of the engine, in stereo, up to 10 minutes.
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
export type InputNames<I extends Inputs, K extends Input["kind"]> = {
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

/** What a sound hears of a field: a number or a boolean as a `Param`, a choice as its value, a file as a `Sample`, a list of numbers as a `Table`. */
type Heard<F> = F extends NumberField | BooleanField
  ? Param
  : F extends ChoiceField<infer Option>
    ? Option
    : F extends FileField
      ? Sample
      : F extends ListField<NumberField>
        ? Table
        : never;

/** What `sound` gets: the fields it can hear, and every input, a bang as a `Param` that is 1 for one sample. */
export type SoundOf<S extends Fields, I extends Inputs> = {
  [Name in keyof S as [Heard<S[Name]>] extends [never] ? never : Name]: Heard<S[Name]>;
} & { [Name in keyof I]: Param };
