// Runs the project's own tools, in Bun, with the `extensions` folder of the project as its
// working folder. It loads the files once: a change of the code starts a new Bun. One JSON
// message per line: the runtime tells on stdin, this says on stdout. What a tool logs goes to
// stderr, so it cannot break a message.

import { realpathSync, writeSync } from "node:fs";
import { join } from "node:path";
import type { Child, Context, Controls, Field, Fields, Graph, HeardEvent, NoteEvent, OwnedChild, Part, Performer, ToolSpec, ValuesOf, Transport } from "./sdk";

const folder = process.cwd();
/** The files of tools to load, by name: the runtime leaves out those whose code hung or stopped Bun. */
const files = process.argv.slice(2);
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

type Spec = ToolSpec<Fields, Controls, unknown>;
type State = ValuesOf<Fields>;
type Choices = Record<string, string | number>;
type Watches = Record<string, number>;
type Size = { width: number; height: number };
/** The room of a page in the check without a window: the default window's. */
const CHECKED_PAGE: Size = { width: 1422, height: 824 };
/** An instance whose code runs, with its watches as they are now. */
type Played = { instance: string; watches: Watches };
/** A record as the runtime tells it: its `state` as the file holds it. */
type Told = { id: string; tool: string; state: unknown };
/** Where the sound and the piece are now, which all code of a tool gets. */
type Clock = { time: number; transport: Transport };

type Request =
  /** The records this Bun did not hold as they are, those that are gone, and how many of its edits the runtime heard. */
  | { type: "records"; records: Told[]; gone: string[]; edits: number }
  /** A card or a page opens: its code makes its nodes. */
  | ({ type: "mount"; card: number; instance: string; watches: Watches; page: Size | null } & Clock)
  | { type: "resize"; card: number; page: Size }
  | { type: "watches"; instance: string; watches: Watches }
  | ({ type: "clock" } & Clock)
  /** A press, a click, or a drag that ended or moved, on a node, with what its handler hears. */
  | { type: "event"; card: number; node: number; kind: "press" | "click" | "end" | "move"; payload: unknown; time: number }
  | ({ type: "frame"; dt: number; instances: Played[] } & Clock)
  | ({ type: "key"; instance: Played; key: string; down: boolean; shift: boolean; alt: boolean; cmd: boolean; ctrl: boolean } & Clock)
  | ({ type: "notes"; instance: Played; events: HeardEvent[] } & Clock)
  | { type: "drop"; card: number }
  | { type: "behaviour"; question: number; tool: string; instance: string };

/** What bindings of an instance read: the window tells this Bun at once only what they read. */
type Reads = { state: string[] | null; children: boolean; watches: boolean; transport: boolean };

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
  name: true, title: true, when: true, doc: true, kind: true, state: true, controls: true, sound: true,
  behaviour: true, children: true, memory: true, tick: true, card: true, page: true, transport: true, onKey: true,
  onNotes: true,
} satisfies Record<keyof Spec, true>;

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
    const names = [...Object.keys(spec.state), ...Object.keys(spec.controls ?? {})];
    for (const name of names) {
      if (!NAME.test(name)) {
        problems.push(`${name}: a field or control name is lowercase letters, digits and _, starting with a letter`);
      }
      if (names.indexOf(name) !== names.lastIndexOf(name)) {
        problems.push(`${name}: a field and a control cannot share a name`);
      }
    }
    for (const name of Object.keys(spec.controls ?? {})) {
      if (name === "audio" || name === "notes") {
        problems.push(`${name}: a control is not named ${name}, the input of the sound`);
      }
    }
    const sounds = choiceFields(spec).reduce((count, [, field]) => count * field.options.length, 1);
    if (sounds > MAX_SOUNDS) {
      problems.push(`its choices make ${sounds} sounds, at most ${MAX_SOUNDS}: make some of them knobs or toggles`);
    }
    if (spec.behaviour && (spec.kind ?? "effect") !== "effect") {
      problems.push('kind: a parent\'s sound hears what it connects to its own { port: "audio" } as `input`, so leave `kind` out');
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
 * and the nodes of its card and page at their defaults, so a card that fails is a problem at
 * once. A function of the wrong type is called anyway, so it fails as a problem of its tool.
 */
function described(spec: Spec, file: string) {
  return {
    info: {
      name: spec.name,
      file,
      title: spec.title,
      when: spec.when,
      doc: spec.doc,
      kind: spec.kind ?? "effect",
      fields: spec.state,
      controls: spec.controls ?? {},
      tick: spec.tick !== undefined,
      page: spec.page !== undefined,
      transport: spec.transport === true,
      keys: spec.onKey !== undefined,
      notes: spec.onNotes !== undefined,
      behaviour: spec.behaviour !== undefined,
      children: spec.children,
    },
    sounds: combinations(spec).map((choices) => [choices, made(() => sound(spec, choices))]),
    card: made(() => draw(spec, false)),
    page: spec.page === undefined ? null : made(() => draw(spec, true)),
  };
}

/** What `make` gives, or why it failed, as the runtime reads a `Result`. */
function made(make: () => unknown) {
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
    fields[name] =
      field.kind === "choice"
        ? (choices[name] ?? field.default)
        : field.kind === "pattern" || field.kind === "sample"
          ? new sdk.Table({ list: name })
          : new sdk.Param(name);
  }
  for (const name of Object.keys(spec.controls ?? {})) {
    fields[name] = new sdk.Param(name);
  }
  const build = spec.sound ?? (() => sdk.input);
  return sdk.graphToJson(() => build(fields as never));
}

/**
 * Answers what the behaviour of a parent declares for its record and its children as the
 * runtime told them, which its question is for: not with the edits this Bun sent since.
 */
function declare({ question, tool, instance }: Extract<Request, { type: "behaviour" }>) {
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
    const answer = spec.behaviour({ state: filledState(spec, record), children: childrenOf(instance, false) });
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
    const declared = JSON.parse(JSON.stringify(answer));
    if (declared !== null && typeof declared === "object" && "players" in declared) {
      throw new Error(
        "`players` are gone: a child of tool `sequencer` loops the patterns now (agent-docs/sequencer.md), " +
          'connected by { from: { child, notes: "notes" }, to: { child, notes: "notes" } }, and set({ child, live: "launch" }, index) launches',
      );
    }
    return declared;
  });
  say({ type: "declared", question, declaration });
}

/** What draws the card of a tool, or its page. */
function drawerOf(spec: Spec, page: boolean) {
  const draw = page ? spec.page : (spec.card ?? sdk.defaultCard(spec.state, spec.controls));
  if (!draw) {
    throw new Error(`${spec.name} has no page`);
  }
  return draw as (card: unknown) => Child;
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

/** What a card, a page and the control loop use to play an instance. */
function performer(instance: string): Performer<Controls> {
  const send: Performer<Controls>["send"] = (event, { at, child } = {}) => {
    checkAt(at);
    say({ type: "send", instance, child, event: wired(event), at });
  };
  return {
    set(control, value) {
      const [child, name] = typeof control === "string" ? [undefined, control] : [control.child, control.live];
      say({ type: "control", instance, child, name, value });
    },
    fire(control, { at } = {}) {
      checkAt(at);
      const [child, name] = typeof control === "string" ? [undefined, control] : [control.child, control.trigger];
      say({ type: "control", instance, child, name, at });
    },
    send,
    play(pitch, { seconds = 0.25, hold = false, velocity = 0.8, at, child } = {}) {
      checkAt(at);
      if (hold) {
        send({ type: "noteOn", pitch, velocity }, { at, child });
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

function hear({ records, gone, edits: heard }: Extract<Request, { type: "records" }>) {
  for (const { id, tool, state } of records) {
    told.set(id, { tool, state });
    const [parent, name] = parentOf(id) ?? [];
    if (parent !== undefined && name !== undefined) {
      kids.set(parent, { names: (kids.get(parent)?.names ?? new Set()).add(name) });
    }
  }
  for (const id of gone) {
    told.delete(id);
    const [parent, name] = parentOf(id) ?? [];
    const known = parent === undefined ? undefined : kids.get(parent);
    if (parent !== undefined && name !== undefined && known) {
      known.names.delete(name);
      kids.set(parent, { names: known.names });
    }
  }
  unheard = unheard.filter(({ number }) => number > heard);
  const touched = new Set<string>();
  for (const id of [...records.map((record) => record.id), ...gone]) {
    touched.add(id);
    const [parent] = parentOf(id) ?? [];
    if (parent !== undefined) touched.add(parent);
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
  // As told: kept until a child changes, so a parent of a thousand takes asks for none of them again.
  if (known && (!latest || unheard.length === 0)) {
    return (known.told ??= listed(id, known.names, false));
  }
  return listed(id, known?.names ?? new Set(), latest);
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
    const entry = recordOf(`${id}/${name}`, latest);
    return entry ? [{ name, ...entry }] : [];
  }));
}

/** The `state` of a record as the code of its tool gets it: every field, at its default where left out. */
function filledState(spec: Spec, entry: Entry): State {
  return frozen(filledObject(spec.state, entry.state));
}

/** Whether a child a change got is a copy whose state it never read. */
const untouched = new WeakMap<object, () => boolean>();

/**
 * A copy of a child for a change to edit, whose state is copied when it is first read. Its name
 * stays: it is the id of the record, which never changes.
 */
function draft(child: OwnedChild): OwnedChild {
  let state: unknown;
  let copied = false;
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
          state = structuredClone(child.state);
          copied = true;
        }
        return state;
      },
      set(value) {
        state = value;
        copied = true;
      },
    },
  });
  untouched.set(copy, () => !copied);
  return copy;
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
  return Object.keys(set).length > 0 || removed.length > 0 ? { set: structuredClone(set), removed } : null;
}

/**
 * What a change made of the record of `instance`, `before` and now `state`, and of its children,
 * `edited` as they were `before` when it read them.
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
  if (children === undefined) {
    return changes;
  }
  if (!Array.isArray(edited)) {
    throw new TypeError("children is a list of { name, tool, state }");
  }
  const old = new Map(children.map((child) => [child.name, child]));
  const kept = new Set<string>();
  for (const child of edited as unknown[]) {
    if (!isObject(child) || typeof child.name !== "string" || typeof child.tool !== "string") {
      throw new TypeError(`a child is { name, tool, state }, not ${JSON.stringify(child)}`);
    }
    const { name, tool } = child;
    if (kept.has(name)) {
      throw new Error(`two children are named ${name}`);
    }
    kept.add(name);
    const id = `${instance}/${name}`;
    const was = old.get(name);
    if (!was || was.tool !== tool) {
      changes.push({ type: "record", id, tool, state: structuredClone(child.state ?? {}) });
    } else if (!untouched.get(child)?.()) {
      const fields = fieldsChanged(was.state, child.state, `the state of ${name}`);
      if (fields) {
        changes.push({ type: "fields", id, ...fields });
      }
    }
  }
  for (const name of old.keys()) {
    if (!kept.has(name)) {
      changes.push({ type: "delete", id: `${instance}/${name}` });
    }
  }
  return changes;
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
    refresh([instance, ...changes.map((change) => parentOf(change.id)?.[0] ?? change.id)], {
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
  { time, transport }: Clock,
  extra: Extra,
) {
  const context = {
    ...extra,
    memory: memoryOf(instance, spec),
    time,
    update: updater(instance),
    now: sdk.now as Context<State>["now"],
    ...performer(instance),
  };
  // Made when code reads them: a tick of a parent of a thousand takes that does not costs nothing for them.
  let children: OwnedChild[] | undefined;
  return Object.defineProperties(context, {
    state: { enumerable: true, value: filledState(spec, record) },
    watches: { enumerable: true, value: watches },
    transport: { enumerable: true, value: transport },
    children: { enumerable: true, get: () => (children ??= childrenOf(instance)) },
  }) as typeof context & { state: State; watches: Watches; transport: Transport; children: OwnedChild[] };
}

// ---------------------------------------------------------------------------------------------
// Cards and pages: their code runs once and makes nodes; stores of the record, its children,
// the watches and the transport change what reads them. The window gets the ops of each card
// after every request, and at once after an `await`.

/** What the cards of an instance read, which its stores hold as they are now. */
type Stores = {
  tool: string;
  state: Record<string, unknown>;
  children: OwnedChild[];
  watches: Watches;
  transport: Transport;
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
let transport: Transport = { playing: false, tick: 0, bar: 1, beat: 1, bpm: 120 };

/** The stores of `instance`, made from the record as code knows it, with its edits not heard. */
function storesOf(instance: string, spec: Spec, record: Entry): Stores {
  let known = stores.get(instance);
  if (!known) {
    known = {
      tool: spec.name,
      state: structuredClone(filledObject(spec.state, record.state)) as Record<string, unknown>,
      children: structuredClone(childrenOf(instance)) as OwnedChild[],
      watches: {},
      transport: { ...transport },
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
  sdk.host.batch(() => {
    for (const known of stores.values()) sdk.host.reconcile(known.transport as never, next as never);
  });
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
      watches: sdk.host.isRead(known.watches),
      transport: sdk.host.isRead(known.transport),
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

/** Opens a card or a page: its code runs once and makes its nodes, or the window hears why not. */
function mount(request: Extract<Request, { type: "mount" }>) {
  const { card, instance, watches, page } = request;
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
  const size = sdk.host.store({ width: page?.width ?? 0, height: page?.height ?? 0 }, false);
  runs(spec.name, page ? "page" : "card");
  try {
    const context = cardContext(instance, spec, known, page ? { size } : {});
    const draw = drawerOf(spec, page !== null);
    const root = sdk.host.mount(card, () => draw(context), { state: context.state, fields: spec.state, update: context.update });
    cards.set(card, { root, instance, tool: spec.name, size: sdk.host.store(size, false) });
    mounting.add(card);
  } catch (error) {
    say({ type: "ops", card, error: String(error), event: false });
  }
}

/** What a card or a page gets: the stores of its instance, its memory, and what plays it. */
function cardContext<Extra extends object>(instance: string, spec: Spec, known: Stores, extra: Extra) {
  const context = {
    ...extra,
    state: sdk.host.store(known.state, false) as State,
    children: sdk.host.store(known.children, false),
    watches: sdk.host.store(known.watches, false),
    transport: sdk.host.store(known.transport, false),
    memory: memoryOf(instance, spec),
    update: updater(instance),
    now: sdk.now as Context<State>["now"],
    ...performer(instance),
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
    case "pattern":
      return Array(field.length).fill(field.default);
    case "sample":
      return "";
    case "notes":
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

/** The ops of a tool's card or page at its defaults, after one tick: a check without a window. */
function draw(spec: Spec, page: boolean): unknown {
  const clock: Clock = { time: 0, transport: { playing: false, tick: 0, bar: 1, beat: 1, bpm: 120 } };
  const memory = freshMemory(spec);
  const performs = { set() {}, fire() {}, play() {}, send() {}, update() {} };
  const state = frozen(filledObject(spec.state, {}));
  const context = { state, children: [], watches: {}, memory, time: 0, transport: clock.transport, now: sdk.now, ...performs };
  try {
    spec.tick?.({ ...context, dt: 1 / 30 } as never);
  } catch (error) {
    throw new Error(`its tick fails: ${error}`);
  }
  const known: Stores = { tool: spec.name, state: structuredClone(state), children: [], watches: {}, transport: clock.transport };
  const drawn = {
    ...context,
    state: sdk.host.store(known.state, false),
    ...(page && { size: sdk.host.store({ ...CHECKED_PAGE }, false) }),
  };
  const root = sdk.host.mount(-1, () => drawerOf(spec, page)(drawn), { state: drawn.state as never, fields: spec.state, update() {} });
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
  call: (spec: Spec, tool: Context<State, Controls, unknown> & Extra) => unknown,
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
      if (mounted) sdk.host.reconcile(mounted.size as never, request.page as never);
      break;
    }
    case "watches":
      setWatches(request.instance, request.watches);
      break;
    case "clock":
      setClock(request);
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
  }
}

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
