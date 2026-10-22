// Tools

import { BangInput, Fields, InputNames, Inputs, NumberField, SoundOf, StateOf, ValuesOf } from "./fields";
import { host } from "./host";
import { AppKey, Clock, KeyName, MenuItem, MenuOptions, Modifiers, MovedNumber, Now, Offers, PlainKey, View } from "./live";
import { Child } from "./nodes";
import { Memory } from "./reactivity";
import { Tools } from "./records";
import { Operand, Stereo } from "./sound";

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
   * (0.25 when left out), or with `hold` only the `noteOn`, which `send("notes", { type:
   * "noteOff", pitch })` or a reload of the code ends. `velocity` is 0.8 when left out.
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
   * Renders the sound of the child at `path`, such as `"bass/chain"`, alone: from `from` to `to`,
   * in ticks, and then what still sounds, such as a reverb, unless `tail` is `false`. Its own
   * output, before what its parents do to it. The file goes into `assets/audio/` named after
   * `name`, `path` when left out: its name there, for a `file` field, or `null` when it failed,
   * which the window says. It reads the records as they are saved.
   */
  render(path: string, options: { from: number; to: number; tail?: boolean; name?: string }): Promise<string | null>;
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
   * the place of the item the composer picked, or `null` when they picked none.
   */
  openMenu(items: readonly MenuItem[], options?: MenuOptions): Promise<number | null>;
  /**
   * Asks the composer `message` with the system's question, its buttons `confirm` and Cancel,
   * such as before what a stray key should not do: whether they said `confirm`.
   */
  confirm(question: { message: string; detail?: string; confirm: string }): Promise<boolean>;
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
