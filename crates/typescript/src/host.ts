// Runs the project's own tools, in Bun, with the `extensions` folder of the project as its
// working folder. It loads the files once: a change of the code starts a new Bun. One JSON
// message per line: the runtime tells on stdin, this says on stdout. What a tool logs goes to
// stderr, so it cannot break a message.

import { realpathSync, writeSync } from "node:fs";
import { join } from "node:path";
import type { Child, Clock as SdkClock, Take, Context, PlainKey, Field, Fields, Graph, HeardEvent, Inputs, MenuItem, MovedNumber, NoteEvent, Offer, OwnedChild, Part, Performer, Section, TempoMap, ToolSpec, ValuesOf } from "./sdk";

const folder = process.cwd();
/** The files of tools to load, by name: the runtime leaves out those whose code hung or stopped Bun. */
const [bunNumber, ...files] = process.argv.slice(2);
/** Each file as a call stack names it, with links resolved. */
const stackPaths = files.map((file) => [file, `${realpathSync(join(folder, file))}:`] as const);
// So the stack `tool` keeps reaches its file through any number of helpers.
Error.stackTraceLimit = Infinity;
const sdk: typeof import("./sdk") = await import(join(folder, "sdk.ts"));

for (const name of ["log", "info", "debug", "warn"] as const) {
  console[name] = (...values: unknown[]) => console.error(...values);
}
// Code of a tool that fails after an `await`, where nothing waits for it, must not stop Bun.
process.on("unhandledRejection", (error) => console.error(error));

type Spec = ToolSpec<Fields, Inputs, unknown>;
type State = ValuesOf<Fields>;
type Choices = Record<string, string | number>;
type Watches = Record<string, number>;
type Size = { width: number; height: number };
/** The room of a view in the check without a window: the default window's. */
const CHECKED_ROOM: Size = { width: 1422, height: 824 };
/** An instance whose code runs, with its watches as they are now. */
type Played = { instance: string; watches: Watches };
/** A record as the runtime tells it: its `state` as the file holds it. */
type Told = { id: string; tool: string; state: unknown };
/** Where the piece plays and how, as the window tells it. */
type Transport = { playing: boolean; tick: number; bar: number; beat: number; bpm: number; repeat: [number, number] | null; click: boolean };
/** Where the sound and the piece are now, which all code of a tool gets. */
type Clock = { time: number; transport: Transport };
/** The tempo map and the sections, as `project.json` holds them. */
type Song = { tempo_map: TempoMap; sections: Section[] };

type Request =
  /** The records this Bun did not hold as they are, those that are gone, and how many of its edits the runtime heard. */
  | { type: "records"; records: Told[]; gone: string[]; edits: number }
  /** A view opens in the room the window gives it: its code makes its nodes. */
  | ({ type: "mount"; card: number; instance: string; watches: Watches; size: Size } & Clock)
  | { type: "resize"; card: number; size: Size }
  | { type: "watches"; instance: string; watches: Watches }
  | ({ type: "clock" } & Clock)
  | ({ type: "song" } & Song)
  /** A press, a click, or a drag that ended or moved, on a node, with what its handler hears. */
  | { type: "event"; card: number; node: number; kind: "press" | "end" | "commit" | "drop" | "power" | "close"; payload: unknown; time: number }
  | ({ type: "frame"; dt: number; instances: Played[] } & Clock)
  | ({ type: "key"; instance: Played; key: PlainKey; down: boolean; shift: boolean; alt: boolean; cmd: boolean; ctrl: boolean } & Clock)
  | ({ type: "notes"; instance: Played; events: HeardEvent[] } & Clock)
  | ({ type: "take"; instance: Played } & Take & Clock)
  | { type: "drop"; card: number }
  | { type: "behaviour"; question: number; tool: string; instance: string }
  /** The answer to a question of `ask`. */
  | { type: "answer"; question: number; answer: unknown };

/** What bindings of an instance read: the window tells this Bun at once only what they read. */
type Reads = { state: string[] | null; children: boolean; watches: string[] | null };

/** Says something to the runtime, at once: before code that may hang runs, and in order. */
function say(message: object) {
  writeSync(1, JSON.stringify(message) + "\n");
}

/** Tells the runtime whose code runs now, so it knows what to blame when it hangs. */
function runs(tool: string, what: string) {
  say({ type: "runs", tool, what });
}

/** What the control loop and the cards of each instance keep. */
const memories = new Map<string, unknown>();
/** The tools whose tick or a click on whose card failed. */
const failed = new Set<string>();

/** Tells the runtime the first failure of a tool: it lists it as a problem. */
function fail(tool: string, message: string) {
  if (!failed.has(tool)) {
    failed.add(tool);
    console.error(`${tool}: ${message}`);
    say({ type: "failed", tool, message });
  }
}

const NAME = /^[a-z][a-z0-9_]*$/;
/** The most combinations of choices a tool has: the sound of each is made when it loads. */
const MAX_SOUNDS = 256;
/** Every key of a tool, so a misspelled or an old one is a problem instead of left out. */
const KEYS = {
  name: true, title: true, when: true, doc: true, voices: true, state: true, inputs: true, sound: true,
  behaviour: true, children: true, memory: true, tick: true, view: true, onKey: true,
  keys: true, globalKeys: true, onNotes: true, onTake: true,
} satisfies Record<keyof Spec, true>;
/** The keys the app keeps, on macOS and elsewhere: no tool takes them. */
const APP_KEYS = ["z", "n", "o", "q", "l", "b", "k"].flatMap((key) => [`cmd-${key}`, `ctrl-${key}`]).concat(["tab"]);
/** The most voices a sound plays at once. */
const MAX_VOICES = 8;

/**
 * What is wrong with the definition of a tool that its types do not say, so the agent that
 * wrote it can fix it. The runtime checks the name, the kinds and the ranges.
 */
function problemsOf(spec: Spec): string[] {
  const problems: string[] = [];
  try {
    for (const key of Object.keys(spec)) {
      if (!(key in KEYS)) {
        problems.push(`${key}: a tool has no ${key}; its keys are ${Object.keys(KEYS).join(", ")}`);
      }
    }
    for (const key of ["title", "when", "doc"] as const) {
      if (!spec[key]?.trim()) {
        problems.push(`${key}: write it, it is what the composer and agents read`);
      }
    }
    const names = [...Object.keys(spec.state), ...Object.keys(spec.inputs ?? {})];
    for (const name of names) {
      if (!NAME.test(name)) {
        problems.push(`${name}: a field or input name is lowercase letters, digits and _, starting with a letter`);
      }
      if (names.indexOf(name) !== names.lastIndexOf(name)) {
        problems.push(`${name}: a field and an input cannot share a name`);
      }
    }
    for (const name of Object.keys(spec.inputs ?? {})) {
      if (name === "audio" || name === "notes") {
        problems.push(`${name}: an input is not named ${name}, an input every sound has`);
      }
    }
    const voices = spec.voices ?? 1;
    if (!Number.isInteger(voices) || voices < 1 || voices > MAX_VOICES) {
      problems.push(`voices: ${voices} is not a whole number from 1 to ${MAX_VOICES}`);
    }
    const sounds = choiceFields(spec).reduce((count, [, field]) => count * field.options.length, 1);
    if (sounds > MAX_SOUNDS) {
      problems.push(`its choices make ${sounds} sounds, at most ${MAX_SOUNDS}: make some of them knobs or toggles`);
    }
    if (spec.behaviour && voices !== 1) {
      problems.push('voices: a parent\'s sound is one voice that hears what it connects to its own { port: "audio" } as `input`, so leave `voices` out');
    }
    for (const [list, names] of [["keys", spec.keys ?? []], ["globalKeys", spec.globalKeys ?? []]] as const) {
      for (const name of names) {
        if (!sdk.host.isKeyName(name)) {
          problems.push(`${list}: ${name} is no key; name one as "a", "space", "left" or "cmd-c", with cmd, ctrl and alt first`);
        }
      }
    }
    for (const key of spec.globalKeys ?? []) {
      if (APP_KEYS.includes(key)) {
        problems.push(`globalKeys: ${key} is the app's: undo, new, open, quit, the panel, the computer keys and tab are`);
      }
    }
    if (spec.children && !spec.behaviour) {
      problems.push("children: only a parent has children: give it a behaviour that connects them");
    }
  } catch (error) {
    // A definition its types refuse, such as one without `state`.
    problems.push(String(error));
  }
  return problems;
}

/** The file of `files` whose code called `tool` for `name`, else `loading`, the file that ran it. */
function definedIn(name: string, loading: string): string {
  for (const frame of (sdk.host.stacks.get(name) ?? "").split("\n")) {
    const found = stackPaths.find(([, path]) => frame.includes(path));
    if (found !== undefined) {
      return found[0];
    }
  }
  return loading;
}

/** Loads the files, each after it says which, so a file that hangs is known. */
async function load() {
  const tools: object[] = [];
  const errors: Array<{ file: string; message: string }> = [];
  for (const file of files) {
    say({ type: "loading", file });
    const before = new Set(sdk.host.tools.keys());
    try {
      await import(join(folder, file));
    } catch (error) {
      // The tools it defined before it failed are its tools too.
      errors.push({ file, message: String(error) });
    }
    for (const [name, spec] of sdk.host.tools) {
      if (before.has(name)) {
        continue;
      }
      const defined = definedIn(name, file);
      const problems = problemsOf(spec);
      errors.push(...problems.map((problem) => ({ file: defined, message: `tool ${name}: ${problem}` })));
      if (problems.length === 0) {
        tools.push(described(spec, defined));
      }
    }
  }
  say({ type: "loaded", tools, errors });
}

/**
 * A tool as the runtime reads it: what it is, the sound of every combination of its choices,
 * and the nodes of its view at its defaults, so a view that fails is a problem at
 * once. A function of the wrong type is called anyway, so it fails as a problem of its tool.
 */
function described(spec: Spec, file: string) {
  const sounds = combinations(spec).map((choices) => [choices, made(() => sound(spec, choices))] as const);
  return {
    info: {
      name: spec.name,
      file,
      title: spec.title,
      when: spec.when,
      doc: spec.doc,
      voices: spec.voices ?? 1,
      input: sounds.some(([, graph]) => "Ok" in graph && readsInput(graph.Ok)),
      sound: spec.sound !== undefined,
      fields: spec.state,
      inputs: spec.inputs ?? {},
      tick: spec.tick !== undefined,
      view: spec.view !== undefined,
      keys: spec.onKey !== undefined,
      takes: spec.keys ?? null,
      global_keys: spec.globalKeys ?? [],
      notes: spec.onNotes !== undefined,
      on_take: spec.onTake !== undefined,
      behaviour: spec.behaviour !== undefined,
      children: spec.children,
    },
    sounds,
    view: made(() => draw(spec)),
  };
}

/** Whether a sound hears what comes in, so its tool has an audio input. */
function readsInput(graph: Graph): boolean {
  return graph.nodes.some((node) => node.op === "input" || node.op === "inputLeft" || node.op === "inputRight");
}

/** What `make` gives, or why it failed, as the runtime reads a `Result`. */
function made<T>(make: () => T): { Ok: T } | { Err: string } {
  try {
    return { Ok: make() };
  } catch (error) {
    return { Err: String(error) };
  }
}

type ChoiceField = Extract<Fields[string], { kind: "choice" }>;

function choiceFields(spec: Spec): Array<[string, ChoiceField]> {
  return Object.entries(spec.state).filter((entry): entry is [string, ChoiceField] => entry[1].kind === "choice");
}

/** Every combination of the choices of a tool. */
function combinations(spec: Spec): Choices[] {
  return choiceFields(spec).reduce<Choices[]>(
    (all, [name, field]) => all.flatMap((choices) => field.options.map((option) => ({ ...choices, [name]: option }))),
    [{}],
  );
}

function specOf(tool: string): Spec {
  const spec = sdk.host.tools.get(tool);
  if (!spec) {
    throw new Error(`no tool ${tool} is loaded`);
  }
  return spec;
}

/** The sound graph of a tool for these choices. */
function sound(spec: Spec, choices: Choices): Graph {
  const fields: Record<string, unknown> = {};
  for (const [name, field] of Object.entries(spec.state)) {
    if (field.kind === "number" || field.kind === "boolean") {
      fields[name] = new sdk.Param(name);
    } else if (field.kind === "choice") {
      fields[name] = choices[name] ?? field.default;
    } else if (field.kind === "file" || (field.kind === "list" && field.item.kind === "number")) {
      fields[name] = new sdk.Table({ list: name });
    }
  }
  for (const name of Object.keys(spec.inputs ?? {})) {
    fields[name] = new sdk.Param(name);
  }
  const build = spec.sound ?? (() => sdk.input);
  return sdk.graphToJson(() => build(fields as never));
}

/**
 * Answers what the behaviour of a parent declares for its record and its children as the
 * runtime told them, which its question is for: not with the edits this Bun sent since.
 */
/**
 * What a behaviour read of its record and its children: the fields of its own state, and the
 * fields of any child's state, each `null` when it read them all, as `Object.keys` does. The
 * names and tools of its children always count. The runtime asks again only when one of these
 * changed, so a fader dragged on a child does not ask a behaviour that reads only names.
 */
type BehaviourReads = { state: string[] | null; children: string[] | null };

/** `source` as a behaviour reads it, which notes each field it reads in `into`. */
function noting<T extends object>(source: T, into: { fields: Set<string> | null }): T {
  const note = (key: PropertyKey) => {
    if (typeof key === "string") into.fields?.add(key);
  };
  const all = () => {
    into.fields = null;
  };
  return new Proxy({} as T, {
    get: (_, key) => (note(key), Reflect.get(source, key)),
    has: (_, key) => (note(key), Reflect.has(source, key)),
    ownKeys: () => (all(), Reflect.ownKeys(source)),
    getOwnPropertyDescriptor: (_, key) => {
      note(key);
      const descriptor = Reflect.getOwnPropertyDescriptor(source, key);
      return descriptor && { ...descriptor, configurable: true };
    },
  });
}

/** The children as a behaviour reads them: every child's state notes the fields it reads. */
/** `children` as a behaviour reads them, at any depth: the fields of the state it reads are what the runtime compares of each. */
function notingChildren(children: OwnedChild[], into: { fields: Set<string> | null }): OwnedChild[] {
  return children.map((child) => {
    let state: unknown;
    let inside: OwnedChild[] | undefined;
    return Object.freeze({
      name: child.name,
      tool: child.tool,
      get state() {
        return (state ??= isObject(child.state) ? noting(child.state, into) : child.state);
      },
      get children() {
        return (inside ??= notingChildren(child.children, into));
      },
    }) as OwnedChild;
  });
}

function declare({ question, tool, instance }: Extract<Request, { type: "behaviour" }>) {
  const own = { fields: new Set<string>() as Set<string> | null };
  const inside = { fields: new Set<string>() as Set<string> | null };
  const declaration = made(() => {
    const spec = specOf(tool);
    if (!spec.behaviour) {
      throw new Error(`${tool} has no behaviour`);
    }
    const record = told.get(instance);
    if (!record) {
      throw new Error(`the runtime told no record of ${instance}`);
    }
    runs(tool, "behaviour");
    const answer = spec.behaviour({
      state: noting(filledState(spec, record), own),
      children: notingChildren(childrenOf(instance, false), inside),
    });
    // JSON leaves out a `child` that is undefined, which would make the end a port of the parent.
    const connections: unknown[] = Array.isArray(answer?.connections) ? answer.connections : [];
    connections.forEach((connection, index) => {
      for (const side of ["from", "to"] as const) {
        const end = (connection as Record<string, unknown> | null)?.[side];
        if (end !== null && typeof end === "object" && "child" in end && typeof end.child !== "string") {
          throw new Error(
            `connections[${index}].${side}.child is ${String(end.child)}: name a child, or leave child out for a port of the parent itself`,
          );
        }
      }
    });
    // Here, so a value JSON cannot hold, such as a cycle, is the parent's problem and does not throw in `say`.
    return JSON.parse(JSON.stringify(answer));
  });
  const list = (read: { fields: Set<string> | null }) => (read.fields ? [...read.fields].sort() : null);
  const reads: BehaviourReads = { state: list(own), children: list(inside) };
  say({ type: "declared", question, declaration, reads });
}

/** What draws the view of a tool. */
function drawerOf(spec: Spec) {
  return (spec.view ?? sdk.defaultView(spec.state, spec.inputs)) as (view: unknown) => Child;
}

/** The memory of an instance, made when it is first needed: a store its cards follow. */
function memoryOf(instance: string, spec: Spec): unknown {
  // A record that becomes another tool's starts its memory again.
  const key = `${instance} ${spec.name}`;
  if (!memories.has(key)) {
    memories.set(key, freshMemory(spec));
  }
  return memories.get(key);
}

function freshMemory(spec: Spec): unknown {
  return sdk.host.store((spec.memory ? spec.memory() : {}) as object, true);
}

/** The `time` of the last frame. */
let now = 0;
/** How far ahead `at` may be: more is a mistake, such as milliseconds, that would hold the list of what is to come. */
const AHEAD = 10;

/** Throws when `at` is too far ahead to be meant. */
function checkAt(at: number | undefined) {
  if (at !== undefined && at > now + AHEAD) {
    throw new RangeError(`at is ${at}, ${(at - now).toFixed(1)} s after time: give a time of the loop in seconds, at most ${AHEAD} s ahead`);
  }
}

/** An input as `send` names it, as the runtime reads it: the child it is of, its name, and what it carries. */
function inputOf(input: unknown): { child?: string; name: string; kind?: "number" | "bang" | "notes" } {
  if (typeof input === "string") return { name: input, kind: input === "notes" ? "notes" : undefined };
  if (input !== null && typeof input === "object" && "child" in input && typeof input.child === "string") {
    for (const kind of ["number", "bang", "notes"] as const) {
      const name = (input as Record<string, unknown>)[kind];
      if (typeof name === "string") return { child: input.child, name, kind };
    }
  }
  throw new TypeError(`send plays an input: its name, "notes", or { child, number }, { child, bang } or { child, notes }, not ${JSON.stringify(input)}`);
}

/** What a view and the control loop use to play an instance. */
function performer(instance: string): Performer<Inputs> {
  const notes = (event: NoteEvent, at: number | undefined, child: string | undefined) => {
    checkAt(at);
    say({ type: "send", instance, child, event: wired(event), at });
  };
  return {
    send: ((input: unknown, ...rest: unknown[]) => {
      const { child, name, kind } = inputOf(input);
      const [first, second] = rest as [unknown, { at?: number } | undefined];
      if (kind === "notes") {
        notes(first as NoteEvent, second?.at, child);
        return;
      }
      // A number input takes a number; a bang takes only when it happens.
      const value = typeof first === "number" ? finite(first, name) : undefined;
      if (kind === "number" && value === undefined) {
        throw new TypeError(`send({ child: "${child}", number: "${name}" }, value) takes a number`);
      }
      const { at } = ((value === undefined ? first : second) ?? {}) as { at?: number };
      checkAt(at);
      say({ type: "control", instance, child, name, value, at });
    }) as Performer<Inputs>["send"],
    play(pitch, { seconds = 0.25, hold = false, velocity = 0.8, at, child } = {}) {
      checkAt(at);
      if (hold) {
        notes({ type: "noteOn", pitch, velocity }, at, child);
      } else {
        say({ type: "note", instance, child, pitch: keyOf(finite(pitch, "pitch")), velocity: finite(velocity, "velocity"), seconds: finite(seconds, "seconds"), at });
      }
    },
  };
}

/** The MIDI key of a pitch. */
function keyOf(pitch: number): number {
  return Math.max(0, Math.min(127, Math.round(pitch)));
}

/** `value` when it is a number JSON can hold, which `NaN` is not. */
function finite(value: number, name: string): number {
  if (!Number.isFinite(value)) {
    throw new RangeError(`${name} is ${value}: give a number`);
  }
  return value;
}

/** A note event as the runtime reads it: only its own keys, and a whole key and controller. */
function wired(event: NoteEvent): NoteEvent {
  switch (event.type) {
    case "noteOn":
      return { type: event.type, pitch: keyOf(finite(event.pitch, "pitch")), velocity: finite(event.velocity, "velocity") };
    case "noteOff":
      return { type: event.type, pitch: keyOf(finite(event.pitch, "pitch")) };
    case "cc": {
      const { controller } = event;
      if (!Number.isInteger(controller) || controller < 0 || controller > 119) {
        throw new RangeError(`controller is ${controller}: a MIDI controller is a whole number from 0 to 119`);
      }
      return { type: event.type, controller, value: finite(event.value, "value") };
    }
    case "bend":
    case "pressure":
      return { type: event.type, value: finite(event.value, "value") };
    case "allOff":
      return { type: event.type };
    default:
      throw new TypeError(`${JSON.stringify(event)} is no note event: its type is noteOn, noteOff, cc, bend, pressure or allOff`);
  }
}

// ---------------------------------------------------------------------------------------------
// The records: what the runtime told, and on top the edits this Bun sent that it has not heard
// yet. What code gets is frozen, so it changes a record only through `update`.

type Entry = { tool: string; state: unknown };
/** One record an edit changes, by its id. The runtime gets the child by name. */
type Change =
  | { type: "fields"; id: string; set: Record<string, unknown>; removed: string[] }
  | { type: "record"; id: string; tool: string; state: unknown }
  | { type: "delete"; id: string };

const told = new Map<string, Entry>();
/** The names of the children of each instance told, and their list as told, made when first read. */
const kids = new Map<string, { names: Set<string>; told?: OwnedChild[] }>();
/** The edits sent that the runtime has not said it heard, oldest first, by their number. */
let unheard: Array<{ number: number; changes: Change[] }> = [];
let edits = 0;

function frozen<T>(value: T): T {
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    for (const each of Object.values(value)) {
      frozen(each);
    }
    Object.freeze(value);
  }
  return value;
}

function parentOf(id: string): [string, string] | undefined {
  const at = id.lastIndexOf("/");
  return at < 0 ? undefined : [id.slice(0, at), id.slice(at + 1)];
}

/** `ids` and every instance they are inside: those whose children change with them. */
function withOwners(ids: Iterable<string>): Set<string> {
  const all = new Set<string>();
  for (const id of ids) {
    all.add(id);
    for (let owner = parentOf(id); owner !== undefined; owner = parentOf(owner[0])) {
      all.add(owner[0]);
    }
  }
  return all;
}

/** Forgets the memory of `id`, which is gone or another tool's now. */
function forget(id: string) {
  for (const key of memories.keys()) {
    if (key.startsWith(`${id} `)) memories.delete(key);
  }
}

function hear({ records, gone, edits: heard }: Extract<Request, { type: "records" }>) {
  for (const { id, tool, state } of records) {
    if (told.get(id)?.tool !== tool) forget(id);
    told.set(id, { tool, state });
    const [parent, name] = parentOf(id) ?? [];
    if (parent !== undefined && name !== undefined) {
      kids.set(parent, { names: (kids.get(parent)?.names ?? new Set()).add(name) });
    }
  }
  for (const id of gone) {
    told.delete(id);
    kids.delete(id);
    forget(id);
    const [parent, name] = parentOf(id) ?? [];
    const known = parent === undefined ? undefined : kids.get(parent);
    if (parent !== undefined && name !== undefined && known) {
      known.names.delete(name);
      kids.set(parent, { names: known.names });
    }
  }
  // An edit the runtime refused changed nothing: what it showed goes back to what was told.
  const refused = unheard.filter(({ number }) => number <= heard).flatMap(({ changes }) => changes.map((change) => change.id));
  unheard = unheard.filter(({ number }) => number > heard);
  const touched = withOwners([...records.map((record) => record.id), ...gone, ...refused]);
  // The lists of children made before hold what changed, also those of an owner far up.
  for (const id of touched) {
    const known = kids.get(id);
    if (known) known.told = undefined;
  }
  refresh(touched);
}

/** The record of `id` as its code knows it: as told, and with the edits not heard on top when `latest`. */
function recordOf(id: string, latest = true): Entry | undefined {
  let entry = told.get(id);
  for (const { changes } of latest ? unheard : []) {
    for (const change of changes) {
      if (change.id !== id) {
        continue;
      }
      if (change.type === "fields") {
        const state: Record<string, unknown> = { ...(entry?.state as object) };
        change.removed.forEach((name) => delete state[name]);
        entry = entry && { tool: entry.tool, state: Object.assign(state, change.set) };
      } else {
        entry = change.type === "record" ? { tool: change.tool, state: change.state } : undefined;
      }
    }
  }
  return entry;
}

/** The children of `id` in name order, as a behaviour gets them. */
function childrenOf(id: string, latest = true): OwnedChild[] {
  const known = kids.get(id);
  // As told: kept until a child changes, so a parent of a thousand takes asks for none of them
  // again, and an edit of one clip makes again only the lists it is in.
  if (known && (!latest || !editedBelow(id))) {
    return (known.told ??= listed(id, known.names, false));
  }
  return listed(id, known?.names ?? new Set(), latest);
}

/** Whether an edit the runtime has not said it heard changes a record inside `id`. */
function editedBelow(id: string): boolean {
  const inside = `${id}/`;
  return unheard.some(({ changes }) => changes.some((change) => change.id.startsWith(inside)));
}

function listed(id: string, told: Set<string>, latest: boolean): OwnedChild[] {
  const names = new Set(told);
  for (const { changes } of latest ? unheard : []) {
    for (const change of changes) {
      const [parent, name] = parentOf(change.id) ?? [];
      if (parent === id && name !== undefined && change.type !== "fields") {
        change.type === "record" ? names.add(name) : names.delete(name);
      }
    }
  }
  return frozen([...names].sort().flatMap((name) => {
    const child = `${id}/${name}`;
    const entry = recordOf(child, latest);
    return entry ? [{ name, tool: entry.tool, state: filledOf(entry), children: childrenOf(child, latest) } as OwnedChild] : [];
  }));
}

/** The `state` of a record as the code of its tool gets it: every field, at its default where left out. */
function filledState(spec: Spec, entry: Entry): State {
  return frozen(filledObject(spec.state, entry.state));
}

/**
 * The state of a child as code gets it: every field that has a default at it, as the code of its
 * tool gets its own, so a view or a change reads `clip.state.start` with no `?? 0`.
 */
function filledOf(child: { tool: string; state: unknown }): unknown {
  const spec = sdk.host.tools.get(child.tool);
  if (spec) return filledObject(spec.state, child.state);
  const defaults = (sdk as { defaultStates?: Record<string, object> }).defaultStates?.[child.tool];
  return defaults && isObject(child.state) ? deepFilled(defaults, child.state) : child.state;
}

/** `state` with what it leaves out of `defaults` filled in, also inside its objects, as Rust reads it. */
function deepFilled(defaults: object, state: Record<string, unknown>): Record<string, unknown> {
  const filled: Record<string, unknown> = structuredClone(defaults) as Record<string, unknown>;
  for (const [name, value] of Object.entries(state)) {
    const fallback = filled[name];
    filled[name] = isObject(value) && isObject(fallback) ? deepFilled(fallback, value) : value;
  }
  return filled;
}

/** What a change did with a child it got: whether it read its state, and its children as it left them. */
type Drafted = { untouched: () => boolean; children: () => [OwnedChild[], unknown] | [] };
const drafts = new WeakMap<object, Drafted>();

/**
 * A copy of a child for a change to edit, whose state, and children, are copied when first
 * read. Its name stays: it is the id of the record, which never changes.
 */
function draft(child: OwnedChild): OwnedChild {
  let state: unknown;
  let copied = false;
  let children: [OwnedChild[], unknown] | [] = [];
  const copy = {} as OwnedChild;
  Object.defineProperties(copy, {
    name: {
      enumerable: true,
      get: () => child.name,
      set() {
        throw new TypeError(`the child ${child.name} keeps its name, the id of its record: keep a title in a field`);
      },
    },
    tool: { enumerable: true, writable: true, value: child.tool },
    state: {
      enumerable: true,
      get() {
        if (!copied) {
          state = structuredClone(filledOf(child));
          copied = true;
        }
        return state;
      },
      set(value) {
        state = value;
        copied = true;
      },
    },
    children: {
      enumerable: true,
      get() {
        if (children.length === 0) children = [child.children, child.children.map(draft)];
        return children[1];
      },
      set(value) {
        children = [child.children, value];
      },
    },
  });
  drafts.set(copy, { untouched: () => !copied, children: () => children });
  return copy;
}

/**
 * A copy of what code put in a record, as JSON: a record holds JSON, and a value read from
 * `memory` or another store is a proxy, which `structuredClone` refuses.
 */
function plain<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/** The fields `after` sets and leaves out against `before`, or `null` when it is the same. */
function fieldsChanged(before: unknown, after: unknown, what: string) {
  if (!isObject(after)) {
    throw new TypeError(`${what} is ${JSON.stringify(after)}: a state is an object of fields`);
  }
  const was = isObject(before) ? before : {};
  const set: Record<string, unknown> = {};
  const removed: string[] = [];
  for (const [name, value] of Object.entries(after)) {
    if (value === undefined) {
      if (name in was) removed.push(name);
    } else if (!(name in was) || JSON.stringify(value) !== JSON.stringify(was[name])) {
      set[name] = value;
    }
  }
  removed.push(...Object.keys(was).filter((name) => !(name in after)));
  return Object.keys(set).length > 0 || removed.length > 0 ? { set: plain(set), removed } : null;
}

/**
 * What a change made of the record of `instance`, `before` and now `state`, and of its children,
 * `edited` as they were `before` when it read them, at any depth.
 */
function changesOf(
  instance: string,
  [before, state]: [State, unknown],
  [children, edited]: [OwnedChild[], unknown] | [],
): Change[] {
  const changes: Change[] = [];
  const own = fieldsChanged(before, state, "state");
  if (own) {
    changes.push({ type: "fields", id: instance, ...own });
  }
  if (children !== undefined) {
    childChanges(instance, children, edited, changes);
  }
  return changes;
}

/** A child of a list a change left, checked: its name, its tool and its state. */
function childOf(child: unknown): OwnedChild {
  if (!isObject(child) || typeof child.name !== "string" || typeof child.tool !== "string") {
    throw new TypeError(`a child is { name, tool, state }, not ${JSON.stringify(child)}`);
  }
  if (child.name.includes("/")) {
    throw new Error(`the child ${child.name} is named with a \`/\`: a grandchild goes in the \`children\` of its parent`);
  }
  return child as OwnedChild;
}

/** What `edited`, the children of `parent` a change left, changes of `children`, as they were. */
function childChanges(parent: string, children: OwnedChild[], edited: unknown, changes: Change[]) {
  if (!Array.isArray(edited)) {
    throw new TypeError("children is a list of { name, tool, state }");
  }
  const old = new Map(children.map((child) => [child.name, child]));
  const kept = new Set<string>();
  for (const each of edited as unknown[]) {
    const child = childOf(each);
    const { name, tool } = child;
    if (kept.has(name)) {
      throw new Error(`two children are named ${name}`);
    }
    kept.add(name);
    const id = `${parent}/${name}`;
    const was = old.get(name);
    if (!was || was.tool !== tool) {
      created(id, child, changes);
      continue;
    }
    const drafted = drafts.get(child);
    if (!drafted?.untouched()) {
      // Against its defaults too: a field the file leaves out and the change did not touch stays out.
      const fields = fieldsChanged(filledOf(was), child.state, `the state of ${name}`);
      if (fields) {
        changes.push({ type: "fields", id, ...fields });
      }
    }
    // A draft's children, when the change read them; the children of a plain object as they are.
    const [owned, changed]: [OwnedChild[] | undefined, unknown] | [] = drafted ? drafted.children() : "children" in child ? [was.children ?? [], child.children] : [];
    if (owned) {
      childChanges(id, owned, changed, changes);
    }
  }
  for (const name of old.keys()) {
    if (!kept.has(name)) {
      // What it owns goes with it.
      changes.push({ type: "delete", id: `${parent}/${name}` });
    }
  }
}

/** A new record at `id` and the children it comes with, parents first. */
function created(id: string, child: OwnedChild, changes: Change[]) {
  changes.push({ type: "record", id, tool: child.tool, state: plain(child.state ?? {}) });
  const inner: unknown = (child as { children?: unknown }).children;
  if (inner === undefined) return;
  if (!Array.isArray(inner)) {
    throw new TypeError(`the children of ${id} are a list of { name, tool, state }`);
  }
  const names = new Set<string>();
  for (const each of inner) {
    const grandchild = childOf(each);
    if (names.has(grandchild.name)) throw new Error(`two children of ${id} are named ${grandchild.name}`);
    names.add(grandchild.name);
    created(`${id}/${grandchild.name}`, grandchild, changes);
  }
}

/**
 * What changes the record of an instance and those of its children, each time on the latest the
 * code knows: also after an `await`, while the instance is there.
 */
function updater(instance: string) {
  return (label: string, change: (part: Part<State>) => void) => {
    if (typeof label !== "string") {
      throw new TypeError(`the label of an update is ${JSON.stringify(label)}: give text, such as "Save the take"`);
    }
    const own = recordOf(instance);
    const spec = sdk.host.tools.get(own?.tool ?? "");
    if (!own || !spec) {
      return;
    }
    const state = filledState(spec, own);
    // The children only when the change reads them: an update of a parent of a thousand takes
    // that edits its own fields costs nothing for them.
    let children: [OwnedChild[], unknown] | [] = [];
    const part = { state: structuredClone(state) } as Part<State>;
    Object.defineProperty(part, "children", {
      enumerable: true,
      get() {
        if (children.length === 0) {
          const before = childrenOf(instance);
          children = [before, before.map(draft)];
        }
        return children[1];
      },
      set(value) {
        children = [children[0] ?? childrenOf(instance), value];
      },
    });
    change(part);
    const changes = changesOf(instance, [state, part.state], children);
    if (changes.length === 0) {
      return;
    }
    const wired = changes.map(({ id, ...change }) => (id === instance ? change : { ...change, child: id.slice(instance.length + 1) }));
    // Before it counts: a value JSON cannot hold, such as a cycle, throws here.
    const line = JSON.stringify({ type: "edit", instance, label, changes: wired }) + "\n";
    edits += 1;
    unheard.push({ number: edits, changes });
    writeSync(1, line);
    // What its cards show follows at once, before the runtime answers.
    refresh(withOwners(changes.map((change) => change.id)), {
      instance,
      state: part.state,
      children: children.length > 0,
    });
  };
}

/** What `tick`, `onKey` and `onNotes` of an instance get: the record as it is now, and its memory. */
function contextOf<Extra extends object>(
  instance: string,
  spec: Spec,
  record: Entry,
  watches: Watches,
  { time }: Clock,
  extra: Extra,
) {
  const context = {
    ...extra,
    time,
    ...doers(instance, spec),
  };
  // Made when code reads them: a tick of a parent of a thousand takes that does not costs nothing for them.
  let children: OwnedChild[] | undefined;
  return Object.defineProperties(context, {
    state: { enumerable: true, value: filledState(spec, record) },
    outputs: { enumerable: true, value: watches },
    children: { enumerable: true, get: () => (children ??= childrenOf(instance)) },
  }) as typeof context & { state: State; outputs: Watches; children: OwnedChild[] };
}

/** What every piece of code of an instance can do, in a view or in `tick`, `onKey`, `onNotes` and `onTake`. */
function doers(instance: string, spec: Spec) {
  return {
    memory: memoryOf(instance, spec),
    clock: clockFor(instance),
    update: updater(instance),
    select: selector(instance),
    addOffer: offerAdder(instance),
    now: sdk.now as Context<State>["now"],
    ...askers(instance),
    ...performer(instance),
  };
}

/** `select`: what the composer works on, a child by its path, which the window plays live notes into. */
function selector(instance: string) {
  return (path: string | null, { span }: { span?: { from: number; to: number } } = {}) => {
    const range = span && [wholeTick(span.from, "span from"), wholeTick(span.to, "span to")];
    say({ type: "select", instance, path, span: range ?? null });
  };
}

/** Questions to the window, by number, which its answers settle: `openFile`, `openMenu`, `offers`, `numbers`. */
const asked = new Map<number, (answer: unknown) => void>();
let questions = Number(bunNumber) * 2 ** 32;
function ask<Answer>(message: { type: string } & Record<string, unknown>): Promise<Answer> {
  const question = ++questions;
  say({ ...message, question });
  return new Promise((resolve) => asked.set(question, resolve as (answer: unknown) => void));
}

/** What a tool asks the window, for its instance. */
function askers(instance: string) {
  return {
    /** The window shows it where the pointer is. */
    openMenu: (items: readonly MenuItem[]) =>
      ask<number | null>({ type: "open_menu", items: items.map(({ label, disabled, checked }) => ({ label, disabled, checked })) }),
    offers: (takes: "notes" | "audio") => ask<Offer[]>({ type: "offers", takes }),
    numbers: (path: string) => ask<MovedNumber[]>({ type: "numbers", instance, path }),
    /** The runtime opens the file and copies it under `assets/audio/`. */
    openFile: ({ title = "Choose a file" }: { title?: string } = {}) => ask<string | null>({ type: "open_file", title }),
  };
}

/** `addOffer`: the window writes the offer's record into the child, as one edit. */
function offerAdder(instance: string) {
  return (key: string, path: string, label: string) => say({ type: "add_offer", instance, path, key, label });
}

// ---------------------------------------------------------------------------------------------
// Views: their code runs once and makes nodes; stores of the record, its children,
// the watches and the clock change what reads them. The window gets the ops of each card
// after every request, and at once after an `await`.

/** What the cards of an instance read, which its stores hold as they are now. */
type Stores = {
  tool: string;
  state: Record<string, unknown>;
  children: OwnedChild[];
  watches: Watches;
  /** What its bindings read as last told to the window, as JSON. */
  told?: string;
};

type Mounted = { root: ReturnType<typeof sdk.host.mount>; instance: string; tool: string; size: Size };

const stores = new Map<string, Stores>();
const cards = new Map<number, Mounted>();
/** Cards whose next ops make all of them again, and the card whose event the next ops answer. */
const mounting = new Set<number>();
const answering = new Set<number>();
let flushing = false;
/** Where the piece plays as last told: what moves on every frame, which no binding follows. */
let transport: Transport = { playing: false, tick: 0, bar: 1, beat: 1, bpm: 120, repeat: null, click: false };
/** What of the clock bindings follow, one for every tool. */
const following = sdk.host.adopted({ playing: false, repeat: null as { from: number; to: number } | null, click: false });
const song = sdk.host.adopted<Song>({ tempo_map: { time_signatures: [{ signature: "4/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 120 }] }, sections: [] });

/** A tick as the window takes it: a whole number from 0. */
function wholeTick(tick: number, what: string): number {
  if (!Number.isFinite(tick) || tick < 0) throw new Error(`${what} ${tick} is not a tick from 0 on`);
  return Math.round(tick);
}

/** The clock as the code of `instance` gets it: what bindings follow, and what moves it. */
function clockFor(instance: string): SdkClock {
  const followed = sdk.host.store(following, false);
  const songOf = sdk.host.store(song, false);
  const moving = (name: "tick" | "bar" | "beat" | "bpm") => {
    if (sdk.host.binding()) {
      throw new Error(`clock.${name} moves on every frame, so no binding follows it: show it with now.tick or now.bpm, or read it in a handler`);
    }
    return transport[name];
  };
  const op = (message: object) => say({ type: "transport", ...message });
  return {
    get playing() { return followed.playing; },
    get repeat() { return followed.repeat; },
    get click() { return followed.click; },
    get tempo_map() { return songOf.tempo_map; },
    get sections() { return songOf.sections; },
    get tick() { return moving("tick"); },
    get bar() { return moving("bar"); },
    get beat() { return moving("beat"); },
    get bpm() { return moving("bpm"); },
    play: () => op({ op: "play" }),
    pause: () => op({ op: "pause" }),
    stop: () => op({ op: "stop" }),
    jump: (to, { atTick } = {}) => op({ op: "jump", to: wholeTick(to, "jump to"), at: atTick === undefined ? null : wholeTick(atTick, "jump atTick") }),
    setRepeat: (range) => op({ op: "repeat", range: range && [wholeTick(range.from, "repeat from"), wholeTick(range.to, "repeat to")] }),
    setClick: (on) => op({ op: "click", on }),
    update: (label, change) => {
      const draft = structuredClone({ tempo_map: song.tempo_map, sections: song.sections });
      change(draft);
      // Before it counts: a value JSON cannot hold throws here.
      const line = JSON.stringify({ type: "song_edit", instance, label, ...draft }) + "\n";
      writeSync(1, line);
      // What its views show follows at once, before the runtime answers.
      sdk.host.batch(() => sdk.host.reconcile(song as never, draft as never));
      later();
    },
  };
}

/** The stores of `instance`, made from the record as code knows it, with its edits not heard. */
function storesOf(instance: string, spec: Spec, record: Entry): Stores {
  let known = stores.get(instance);
  // A record of another tool now is another instance to its code.
  if (!known || known.tool !== spec.name) {
    known = {
      tool: spec.name,
      state: sdk.host.adopted(filledState(spec, record)) as Record<string, unknown>,
      children: sdk.host.adopted(childrenOf(instance)) as OwnedChild[],
      watches: {},
    };
    stores.set(instance, known);
  }
  return known;
}

/**
 * Brings the stores of each of `instances` to the records as code knows them now; `edited` is
 * the state an `update` just made, which is that already.
 */
function refresh(instances: Iterable<string>, edited?: { instance: string; state: unknown; children: boolean }) {
  sdk.host.batch(() => {
    for (const instance of new Set(instances)) {
      const known = stores.get(instance);
      const record = recordOf(instance);
      const spec = sdk.host.tools.get(known?.tool ?? "");
      if (known && (!record || record.tool !== known.tool)) stores.delete(instance);
      if (!known || !record || !spec || record.tool !== known.tool) continue;
      const own = edited?.instance === instance ? edited : undefined;
      sdk.host.reconcile(known.state, (own?.state ?? filledState(spec, record)) as Record<string, unknown>);
      if (!own || own.children) {
        sdk.host.reconcile(known.children as never, childrenOf(instance) as never);
      }
    }
  });
  later();
}

function setWatches(instance: string, watches: Watches) {
  const known = stores.get(instance);
  if (known) sdk.host.reconcile(known.watches, watches);
}

function setClock({ time, transport: next }: Clock) {
  now = time;
  transport = next;
  const repeat = next.repeat && { from: next.repeat[0], to: next.repeat[1] };
  sdk.host.batch(() => sdk.host.reconcile(following as never, { playing: next.playing, repeat, click: next.click } as never));
}

/** Sends the ops at once when code changed something outside a request, such as after an `await`. */
function later() {
  if (!flushing) {
    flushing = true;
    queueMicrotask(flushOps);
  }
}

/** Sends the window the ops of every card that has some, and what the bindings of each instance read now. */
function flushOps() {
  flushing = false;
  for (const [card, mounted] of cards) {
    const taken = sdk.host.takeOps(mounted.root);
    const mount = mounting.delete(card);
    const event = answering.delete(card);
    if ("error" in taken) {
      fail(mounted.tool, `its card failed: ${taken.error}`);
      say({ type: "ops", card, error: taken.error, event });
      mounted.root.dispose();
      cards.delete(card);
    } else if (taken.ops.length > 0 || mount || event) {
      const started = performance.now();
      const text = JSON.stringify(taken.ops);
      writeSync(1, `{"type":"ops","card":${card},"mount":${mount},"event":${event},"took":${performance.now() - started + took},"ops":${text}}\n`);
    }
  }
  for (const card of answering) {
    say({ type: "ops", card, mount: false, event: true, took: 0, ops: [] });
  }
  answering.clear();
  for (const [instance, known] of stores) {
    const reads: Reads = {
      state: sdk.host.readFields(known.state),
      children: sdk.host.isRead(known.children),
      watches: sdk.host.readFields(known.watches),
    };
    const text = JSON.stringify(reads);
    if (text !== known.told) {
      known.told = text;
      say({ type: "reads", instance, reads });
    }
  }
}

/** Milliseconds this Bun spent on the request whose ops go now. */
let took = 0;

/** Opens a view: its code runs once and makes its nodes, or the window hears why not. */
function mount(request: Extract<Request, { type: "mount" }>) {
  const { card, instance, watches } = request;
  setClock(request);
  const record = recordOf(instance);
  const spec = sdk.host.tools.get(record?.tool ?? "");
  if (!record || !spec) {
    say({ type: "ops", card, error: record ? `no tool ${record.tool} is loaded` : "drawn without a record", event: false });
    return;
  }
  cards.get(card)?.root.dispose();
  cards.delete(card);
  const known = storesOf(instance, spec, record);
  setWatches(instance, watches);
  // A resize reconciles the object itself: the store over it is read only.
  const room = { ...request.size };
  const size = sdk.host.store(room, false);
  runs(spec.name, "view");
  try {
    const context = cardContext(instance, spec, known, { size });
    const draw = drawerOf(spec);
    const root = sdk.host.mount(card, () => draw(context), { state: context.state, fields: spec.state, update: context.update });
    cards.set(card, { root, instance, tool: spec.name, size: room });
    mounting.add(card);
  } catch (error) {
    say({ type: "ops", card, error: String(error), event: false });
  }
}

/** What a view gets: the stores of its instance, its memory, and what plays it. */
function cardContext<Extra extends object>(instance: string, spec: Spec, known: Stores, extra: Extra) {
  const context = {
    ...extra,
    state: sdk.host.store(known.state, false) as State,
    children: sdk.host.store(known.children, false),
    outputs: sdk.host.store(known.watches, false),
    ...doers(instance, spec),
  };
  return Object.defineProperty(context, "time", { enumerable: true, get: () => now }) as typeof context & { time: number };
}

/** `value` of `field` as the window gives it: at its default when left out, and so each item. */
function filled(field: Field, value: unknown): unknown {
  const given = value ?? defaultOf(field);
  if (field.kind !== "list" || !Array.isArray(given)) {
    return given;
  }
  const item = field.item;
  return given.map((each) => (item.kind === "object" ? filledObject(item.fields, each) : filled(item, each)));
}

function filledObject(fields: Fields, value: unknown): State {
  const object = (value ?? {}) as Record<string, unknown>;
  return Object.fromEntries(Object.entries(fields).map(([name, field]) => [name, filled(field, object[name])])) as State;
}

function defaultOf(field: Field): unknown {
  switch (field.kind) {
    case "file":
      return "";
    case "list":
      return field.default ?? [];
    case "text":
      return field.default ?? "";
    case "number":
      return field.default ?? field.min;
    default:
      return field.default;
  }
}

/** The ops of a tool's view at its defaults, after one tick: a check without a window. */
function draw(spec: Spec): unknown {
  const memory = freshMemory(spec);
  const performs = { play() {}, send() {}, update() {}, select() {}, openFile: async () => null, openMenu: async () => null, offers: async () => [], numbers: async () => [], addOffer() {} };
  const state = frozen(filledObject(spec.state, {}));
  // What the check plays on the clock goes nowhere.
  const clock = { ...clockFor(""), play() {}, pause() {}, stop() {}, jump() {}, setRepeat() {}, setClick() {}, update() {} };
  const context = { state, children: [], outputs: {}, memory, time: 0, clock, now: sdk.now, ...performs };
  try {
    spec.tick?.({ ...context, dt: 1 / 30 } as never);
  } catch (error) {
    throw new Error(`its tick fails: ${error}`);
  }
  const known: Stores = { tool: spec.name, state: structuredClone(state), children: [], watches: {} };
  const drawn = { ...context, state: sdk.host.store(known.state, false), size: sdk.host.store({ ...CHECKED_ROOM }, false) };
  const root = sdk.host.mount(-1, () => drawerOf(spec)(drawn), { state: drawn.state as never, fields: spec.state, update() {} });
  const taken = sdk.host.takeOps(root);
  root.dispose();
  if ("error" in taken) {
    throw new Error(taken.error);
  }
  return taken.ops;
}

/**
 * Runs the code of each instance that `call` picks, with what all its code gets; its cards
 * follow what it changed. `what` names that code in a problem.
 */
function run<Extra extends object>(
  // Only the clock: a key or notes request has an `instance` of its own, which the cards would draw.
  { time, transport }: Clock,
  instances: Played[],
  what: string,
  extra: Extra,
  call: (spec: Spec, tool: Context<State, Inputs, unknown> & Extra) => unknown,
) {
  const clock = { time, transport };
  setClock(clock);
  for (const { instance, watches } of instances) {
    setWatches(instance, watches);
    const record = recordOf(instance);
    const spec = sdk.host.tools.get(record?.tool ?? "");
    if (!spec || !record) {
      continue;
    }
    const failed = (error: unknown) => fail(spec.name, `its ${what} failed: ${error}`);
    try {
      runs(spec.name, what);
      caught(call(spec, contextOf(instance, spec, record, watches, clock, extra)), failed);
    } catch (error) {
      failed(error);
    }
  }
}

/** Hears the failure of code that went on after an `await`. */
function caught(result: unknown, failed: (error: unknown) => void) {
  if (result instanceof Promise) {
    result.catch(failed);
  }
}

function handle(request: Request) {
  switch (request.type) {
    case "records":
      hear(request);
      break;
    case "mount":
      mount(request);
      break;
    case "resize": {
      const mounted = cards.get(request.card);
      if (mounted) sdk.host.reconcile(mounted.size as never, request.size as never);
      break;
    }
    case "watches":
      setWatches(request.instance, request.watches);
      break;
    case "clock":
      setClock(request);
      break;
    case "song":
      sdk.host.reconcile(song as never, { tempo_map: request.tempo_map, sections: request.sections } as never);
      break;
    case "event": {
      now = request.time;
      const mounted = cards.get(request.card);
      const failed = (error: unknown) => mounted && fail(mounted.tool, `a click on its card failed: ${error}`);
      try {
        if (mounted) {
          runs(mounted.tool, "click handler");
        }
        caught(sdk.host.batch(() => sdk.host.nodeEvent(request.node, request.kind, request.payload as never)), failed);
      } catch (error) {
        failed(error);
      }
      answering.add(request.card);
      break;
    }
    case "frame":
      run(request, request.instances, "tick", { dt: request.dt }, (spec, tool) => spec.tick?.(tool));
      say({ type: "framed" });
      break;
    case "key": {
      const { key: name, down, shift, alt, cmd, ctrl } = request;
      const key = { key: name, down, shift, alt, cmd, ctrl };
      run(request, [request.instance], "onKey", {}, (spec, tool) => spec.onKey?.(tool, key));
      break;
    }
    case "take": {
      const { recorder, name, tool: kind, state } = request;
      run(request, [request.instance], "onTake", {}, (spec, tool) => spec.onTake?.(tool, { recorder, name, tool: kind, state } as Take));
      break;
    }
    case "notes":
      run(request, [request.instance], "onNotes", {}, (spec, tool) =>
        Promise.all(request.events.map((event) => spec.onNotes?.(tool, event))),
      );
      break;
    case "drop":
      cards.get(request.card)?.root.dispose();
      cards.delete(request.card);
      mounting.delete(request.card);
      break;
    case "behaviour":
      declare(request);
      break;
    case "answer":
      asked.get(request.question)?.(request.answer);
      asked.delete(request.question);
      break;
  }
}

sdk.host.changed = later;

await load();

for await (const line of console) {
  // Bun gives the end of the input after the last newline as one more, empty, line.
  if (line === "") {
    continue;
  }
  const started = performance.now();
  try {
    sdk.host.batch(() => handle(JSON.parse(line) as Request));
  } finally {
    took = performance.now() - started;
    flushOps();
    // Bun does one thing at a time, so the runtime knows what hangs: the oldest not done.
    say({ type: "done" });
  }
}
// The runtime closed stdin: it quit.
process.exit(0);
