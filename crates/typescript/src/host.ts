// Runs the project's own tools, in Bun, with the `extensions` folder of the project as its
// working folder. It loads the files once: a change of the code starts a new Bun. One JSON
// message per line: the runtime tells on stdin, this says on stdout. What a tool logs goes to
// stderr, so it cannot break a message.

import { realpathSync, writeSync } from "node:fs";
import { join } from "node:path";
import type { Child, Context, Controls, Drag, Field, Fields, Graph, HeardEvent, Node, NoteEvent, OwnedChild, Part, Performer, ToolSpec, ValuesOf, Transport } from "./sdk";

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

type Render = { type: "render"; card: number; instance: string; watches: Watches; page: Size | null } & Clock;
type Request =
  /** The records this Bun did not hold as they are, those that are gone, and how many of its edits the runtime heard. */
  | { type: "records"; records: Told[]; gone: string[]; edits: number }
  | Render
  /** A press, or a drag that ended or moved, with what its handler hears. */
  | { type: "event"; card: number; version: number; handler: number; payload: unknown; time: number }
  | ({ type: "frame"; dt: number; instances: Played[] } & Clock)
  | ({ type: "key"; instance: Played; key: string; down: boolean; shift: boolean; alt: boolean; cmd: boolean; ctrl: boolean } & Clock)
  | ({ type: "notes"; instance: Played; events: HeardEvent[] } & Clock)
  | { type: "drop"; card: number }
  | { type: "behaviour"; question: number; tool: string; instance: string };

/** What the runtime gets of a node or a shape: a handler is its index. */
type Sent = { [key: string]: unknown };

/** A press or a drag, which hears what the window says of it. */
type Handler = (payload: never) => unknown;

/** What the code of a card read as it drew: it draws again only when one of those changes. */
type Reads = { state: Set<string>; all: boolean; children: boolean; childFields: Set<string>; childAll: boolean; watches: boolean; transport: boolean };

/** Says something to the runtime, at once: before code that may hang runs, and in order. */
function say(message: object) {
  writeSync(1, JSON.stringify(message) + "\n");
}

/** Tells the runtime whose code runs now, so it knows what to blame when it hangs. */
function runs(tool: string, what: string) {
  say({ type: "runs", tool, what });
}

/** The handlers of the last trees of each card, by the version of the tree. */
const handlers = new Map<number, Map<number, Handler[]>>();
/**
 * The version of the last tree drawn: a click names the tree it was on. From the time this Bun
 * started, so a click on a tree an earlier Bun drew runs nothing here.
 */
let treeVersion = Date.now() * 1000;
/** The trees of a card whose clicks still count: the window may still show an older one. */
const KEPT_TREES = 8;
/** The last tree sent of each card, so the same tree again is not sent. */
const lastTrees = new Map<number, { text: string; version: number }>();
/** What each card was last drawn from, so it draws again after a tick or a click. */
const drawn = new Map<number, Render & { tool: string }>();
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
 * and its card and page at their defaults, so the runtime never waits for Bun. A function of
 * the wrong type is called anyway, so it fails as a problem of its tool.
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

function flatten(child: Child, handlers: Kept, into: Sent[]) {
  if (child === null || child === undefined || typeof child === "boolean") {
    return;
  }
  if (Array.isArray(child)) {
    for (const each of child) {
      flatten(each, handlers, into);
    }
  } else if (typeof child === "string" || typeof child === "number") {
    // `{count} lines` is three children in JSX and one line of text on screen.
    const last = into.at(-1);
    if (last?.type === "text") {
      last.text += String(child);
    } else {
      into.push({ type: "text", text: String(child) });
    }
  } else {
    into.push(serialize(child, handlers));
  }
}

/** The handlers of a tree, by their index, which the runtime names; a handler used twice is kept once. */
type Kept = { list: Handler[]; index: Map<Handler, number> };

function kept(): Kept {
  return { list: [], index: new Map() };
}

/** A node as the runtime gets it, with its handlers kept for its card. */
function serialize(node: Node, handlers: Kept): Sent {
  const keep = (handler: Handler | undefined) => {
    if (!handler) {
      return undefined;
    }
    let index = handlers.index.get(handler);
    if (index === undefined) {
      index = handlers.list.push(handler) - 1;
      handlers.index.set(handler, index);
    }
    return index;
  };
  const drag = (drag: Drag | undefined) => drag && { ...drag, onEnd: keep(drag.onEnd), onMove: keep(drag.onMove) };
  if ("kind" in node) {
    throw new Error(`a ${node.kind} is a shape: put it in a Space`);
  }
  if (node.type === "space") {
    // A drag many shapes share, such as the one of every note, goes once: shapes name it by its place.
    const drags: Sent[] = [];
    const dragIndex = new Map<Drag, number>();
    const shared = (shape: Drag | undefined) => {
      if (!shape) {
        return undefined;
      }
      let index = dragIndex.get(shape);
      if (index === undefined) {
        index = drags.push(drag(shape) as Sent) - 1;
        dragIndex.set(shape, index);
      }
      return index;
    };
    const collect = (child: Child, into: Sent[]) => {
      if (Array.isArray(child)) {
        for (const each of child) {
          collect(each, into);
        }
      } else if (child !== null && typeof child === "object" && "kind" in child) {
        if (child.kind === "group") {
          const shapes: Sent[] = [];
          collect(child.children, shapes);
          into.push({ ...child, children: shapes });
        } else {
          into.push({ ...child, onPress: keep(child.onPress), drag: shared(child.drag) });
        }
      } else if (child !== null && child !== undefined && typeof child !== "boolean") {
        throw new Error(`a Space draws shapes such as Rect and Label, not ${typeof child === "object" && "type" in child ? `a ${child.type}` : JSON.stringify(child)}`);
      }
    };
    const shapes: Sent[] = [];
    collect(node.children, shapes);
    return { ...node, onPress: keep(node.onPress), drag: drag(node.drag), drags, children: shapes };
  }
  if (node.type !== "div") {
    return node;
  }
  const children: Sent[] = [];
  flatten(node.children, handlers, children);
  return { type: "div", key: node.key, style: node.style, onClick: keep(node.onClick), children };
}

/** What draws the card of a tool, or its page. */
function drawerOf(spec: Spec, page: boolean) {
  const draw = page ? spec.page : (spec.card ?? sdk.defaultCard(spec.state, spec.controls));
  if (!draw) {
    throw new Error(`${spec.name} has no page`);
  }
  return draw as (card: unknown) => Node;
}

/** The memory of an instance, made when it is first needed. */
function memoryOf(instance: string, spec: Spec): unknown {
  // A record that becomes another tool's starts its memory again.
  const key = `${instance} ${spec.name}`;
  if (!memories.has(key)) {
    memories.set(key, spec.memory ? spec.memory() : {});
  }
  return memories.get(key);
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
  };
}

/** What all code of an instance gets: a card, a page, `tick`, `onKey` and `onNotes`. */
function contextOf<Extra extends object>(
  instance: string,
  spec: Spec,
  record: Entry,
  watches: Watches,
  { time, transport }: Clock,
  extra: Extra,
  reads?: Reads,
) {
  const context = {
    ...extra,
    memory: memoryOf(instance, spec),
    time,
    update: updater(instance),
    now: sdk.now as Context<State>["now"],
    ...performer(instance),
  };
  const filled = filledState(spec, record);
  const state = reads ? tracked(filled, reads) : filled;
  // Made when code reads them: a tick of a parent of a thousand takes that does not costs nothing for them.
  let children: OwnedChild[] | undefined;
  const read = <T>(what: "children" | "watches" | "transport", value: () => T) => ({
    enumerable: true,
    get: () => {
      if (reads) {
        reads[what] = true;
      }
      return value();
    },
  });
  return Object.defineProperties(context, {
    state: { enumerable: true, value: state },
    watches: read("watches", () => watches),
    transport: read("transport", () => transport),
    children: read("children", () => (children ??= reads ? childrenOf(instance).map((child) => trackedChild(child, reads)) : childrenOf(instance))),
  }) as typeof context & { state: State; watches: Watches; transport: Transport; children: OwnedChild[] };
}

/** A child whose `state` notes in `reads` each field code reads of it, as `child.field`. */
function trackedChild(child: OwnedChild, reads: Reads): OwnedChild {
  const state = new Proxy(child.state as object, {
    get(target, name, receiver) {
      if (typeof name === "string") {
        reads.childFields.add(`${child.name}.${name}`);
      }
      return Reflect.get(target, name, receiver);
    },
    has(target, name) {
      if (typeof name === "string") {
        reads.childFields.add(`${child.name}.${name}`);
      }
      return Reflect.has(target, name);
    },
    ownKeys(target) {
      reads.childAll = true;
      return Reflect.ownKeys(target);
    },
  });
  return { ...child, state } as OwnedChild;
}

/** `state` that notes in `reads` each field code reads of it. */
function tracked(state: State, reads: Reads): State {
  return new Proxy(state, {
    get(target, name, receiver) {
      if (typeof name === "string") {
        reads.state.add(name);
      }
      return Reflect.get(target, name, receiver);
    },
    has(target, name) {
      if (typeof name === "string") {
        reads.state.add(name);
      }
      return Reflect.has(target, name);
    },
    ownKeys(target) {
      reads.all = true;
      return Reflect.ownKeys(target);
    },
  });
}

/**
 * Draws a card and sends its tree, or why there is none: the window waits for one or the other.
 * With what its code read, so the window asks again only when that changed; and `event` after
 * a press or a drag, whose preview the window drops with this tree.
 */
function render(request: Render, event = false) {
  const { card, instance, watches, page } = request;
  now = request.time;
  const record = recordOf(instance);
  if (!record) {
    say({ type: "tree", card, tree: { Err: "drawn without a record" } });
    return;
  }
  const { tool } = record;
  drawn.set(card, { ...request, tool });
  let same = false;
  const reads: Reads = { state: new Set(), all: false, children: false, childFields: new Set(), childAll: false, watches: false, transport: false };
  const tree = made(() => {
    // Such as a tool whose file this Bun runs without.
    const spec = specOf(tool);
    const drawnHandlers = kept();
    runs(tool, page ? "page" : "card");
    const context = contextOf(instance, spec, record, watches, request, page ? { size: page } : {}, reads);
    const drawnTree = drawing(spec, context, () => serialize(drawerOf(spec, page !== null)(context), drawnHandlers));
    const text = JSON.stringify(drawnTree);
    const trees = handlers.get(card) ?? new Map<number, Handler[]>();
    const last = lastTrees.get(card);
    // The same tree keeps its version, with the handlers of this draw.
    if (last?.text === text) {
      trees.set(last.version, drawnHandlers.list);
      same = true;
      return null;
    }
    treeVersion += 1;
    lastTrees.set(card, { text, version: treeVersion });
    trees.set(treeVersion, drawnHandlers.list);
    if (trees.size > KEPT_TREES) {
      const [oldest] = trees.keys();
      trees.delete(oldest);
    }
    handlers.set(card, trees);
    return [treeVersion, text] as const;
  });
  if ("Err" in tree) {
    lastTrees.delete(card);
  }
  const read = {
    state: reads.all ? null : [...reads.state],
    children: reads.children,
    child_fields: reads.childAll ? null : [...reads.childFields],
    watches: reads.watches,
    transport: reads.transport,
  };
  if (same) {
    say({ type: "unchanged", card, reads: read, event });
  } else if ("Err" in tree) {
    say({ type: "tree", card, tree, reads: read, event });
  } else {
    // The tree is JSON already: a large one is not written out twice.
    const [version, text] = tree.Ok as readonly [number, string];
    writeSync(1, `{"type":"tree","card":${card},"tree":{"Ok":[${version},${text}]},"reads":${JSON.stringify(read)},"event":${event}}\n`);
  }
}

/** Runs `draw` as the card of `context` draws, for the components of the SDK that read it. */
function drawing<T>(spec: Spec, context: { state: State; update: Context<State>["update"] }, draw: () => T): T {
  sdk.host.drawing = { state: context.state, fields: spec.state, update: context.update };
  try {
    return draw();
  } finally {
    sdk.host.drawing = undefined;
  }
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

/** The tree of a tool's card or page at its defaults, after one tick: a check without a window. */
function draw(spec: Spec, page: boolean): Sent {
  const context = {
    state: frozen(filledObject(spec.state, {})),
    children: [],
    watches: {},
    memory: spec.memory ? spec.memory() : {},
    time: 0,
    transport: { playing: false, tick: 0, bar: 1, beat: 1, bpm: 120 },
    set() {},
    fire() {},
    play() {},
    send() {},
    update() {},
    now: sdk.now as Context<State>["now"],
  };
  try {
    spec.tick?.({ ...context, dt: 1 / 30 });
  } catch (error) {
    throw new Error(`its tick fails: ${error}`);
  }
  const drawn = { ...context, ...(page && { size: CHECKED_PAGE }) };
  return drawing(spec, drawn, () => serialize(drawerOf(spec, page)(drawn), kept()));
}

/**
 * Runs the code of each instance that `call` picks, with what all its code gets, then its cards
 * draw again. `what` names that code in a problem.
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
  now = time;
  for (const { instance, watches } of instances) {
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
      continue;
    }
    for (const last of drawn.values()) {
      if (last.instance === instance) {
        render({ ...last, ...clock, watches });
      }
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
    case "render":
      render(request);
      break;
    case "event": {
      now = request.time;
      const last = drawn.get(request.card);
      const failed = (error: unknown) => last && fail(last.tool, `a click on its card failed: ${error}`);
      try {
        if (last) {
          runs(last.tool, "click handler");
        }
        const handler = handlers.get(request.card)?.get(request.version)?.[request.handler] as ((payload: unknown) => unknown) | undefined;
        caught(handler?.(request.payload), failed);
      } catch (error) {
        failed(error);
      }
      // It may have changed the memory.
      if (last) {
        render(last, true);
      }
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
      handlers.delete(request.card);
      lastTrees.delete(request.card);
      drawn.delete(request.card);
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
  try {
    handle(JSON.parse(line) as Request);
  } finally {
    // Bun does one thing at a time, so the runtime knows what hangs: the oldest not done.
    say({ type: "done" });
  }
}
// The runtime closed stdin: it quit.
process.exit(0);
