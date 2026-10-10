// Runs the project's own tools, in Bun, with the `extensions` folder of the project as its
// working folder. One JSON message per line: the runtime asks on stdin, this answers on
// stdout. What a tool logs goes to stderr, so it cannot break a message.

import { readdirSync, watch } from "node:fs";
import { join } from "node:path";
import type { Child, Controls, Fields, Graph, Handler as Heard, Midi, Node, Performer, StateOf, ToolSpec } from "./sdk";

const folder = process.cwd();
const sdk: typeof import("./sdk") = await import(join(folder, "sdk.ts"));

for (const name of ["log", "info", "debug", "warn"] as const) {
  console[name] = (...values: unknown[]) => console.error(...values);
}

type Spec = ToolSpec<Fields, Controls, unknown>;
type State = StateOf<Fields>;
type Watches = Record<string, number>;
type Size = { width: number; height: number };
/** The room of a page in the check without a window: the default window's. */
const CHECKED_PAGE: Size = { width: 1422, height: 824 };
/** An instance as it is now, for its control loop or what it hears: `edits` counts the edits from here its `state` has. */
type Looped = { instance: string; tool: string; state: State; watches: Watches; edits: number };

type Render = { type: "render"; card: number; instance: string; tool: string; state: State; watches: Watches; page: Size | null };
type Request =
  | { type: "sound"; id: number; tool: string; choices: Record<string, string | number> }
  | { type: "draw"; id: number; tool: string; page: boolean }
  | Render
  /** A click, or a press or a drag on a canvas `at` across and down. */
  | { type: "event"; card: number; version: number; handler: number; at: [number, number] | null }
  | { type: "frame"; dt: number; time: number; instances: Looped[] }
  | { type: "key"; time: number; instance: Looped; key: string; down: boolean }
  | { type: "midi"; time: number; instances: Looped[]; messages: Midi[] }
  | { type: "drop"; card: number };

type Canvas = Extract<Node, { type: "canvas" }>;
/** What the runtime gets of a node: a handler is its index. */
type Sent =
  | { type: "div"; style?: unknown; onClick?: number; children: Sent[] }
  | { type: "text"; text: string }
  | (Omit<Canvas, "onPress" | "onDrag"> & { onPress?: number; onDrag?: number })
  | Exclude<Node, { type: "div" | "canvas" }>;

/** A click, which hears nothing, or a press or a drag on a canvas, which hears where. */
type Handler = (x: number, y: number) => void;

function send(message: object) {
  process.stdout.write(JSON.stringify(message) + "\n");
}

/** The handlers of the last trees of each card, by the version of the tree. */
const handlers = new Map<number, Map<number, Handler[]>>();
/** The version of the last tree drawn: a click names the tree it was on. */
let treeVersion = 0;
/** The trees of a card whose clicks still count: the window may still show an older one. */
const KEPT_TREES = 8;
/** What each card was last drawn from, so it draws again after a tick or a click. */
const drawn = new Map<number, Render>();
/** What the control loop and the cards of each instance keep. */
const memories = new Map<string, unknown>();
/** The tools whose tick or a click on whose card failed since the last load. */
const failed = new Set<string>();

/** Tells the runtime the first failure of a tool since the last load: it lists it as a problem. */
function fail(tool: string, message: string) {
  if (!failed.has(tool)) {
    failed.add(tool);
    console.error(`${tool}: ${message}`);
    send({ type: "failed", tool, message });
  }
}
let version = 0;

const NAME = /^[a-z][a-z0-9_]*$/;

/**
 * What is wrong with the definition of a tool that its types do not say, so the agent that
 * wrote it can fix it. The runtime checks the name and the kinds.
 */
function problemsOf(spec: Spec): string[] {
  const problems: string[] = [];
  try {
    for (const key of ["title", "when", "doc"] as const) {
      if (!spec[key]?.trim()) {
        problems.push(`${key}: write it, it is what the composer and agents read`);
      }
    }
    const range = (at: string, min: number, max: number, value: number) => {
      if (!(min < max)) problems.push(`${at}: min must be below max`);
      if (!(value >= min && value <= max)) {
        problems.push(`${at}: default ${value} is outside [${min}, ${max}]`);
      }
    };
    const names = [...Object.keys(spec.state), ...Object.keys(spec.controls ?? {})];
    for (const name of names) {
      if (!NAME.test(name)) {
        problems.push(`${name}: a field or control name is lowercase letters, digits and _, starting with a letter`);
      }
      if (names.indexOf(name) !== names.lastIndexOf(name)) {
        problems.push(`${name}: a field and a control cannot share a name`);
      }
    }
    for (const [name, field] of Object.entries(spec.state)) {
      if (field.kind === "knob" || field.kind === "pattern") {
        range(`state.${name}`, field.min, field.max, field.default);
      }
      if (field.kind === "pattern" && !(Number.isInteger(field.length) && field.length >= 1 && field.length <= 1024)) {
        problems.push(`state.${name}: a pattern is 1 to 1024 long`);
      }
    }
    for (const [name, control] of Object.entries(spec.controls ?? {})) {
      if (control.kind === "live") {
        range(`controls.${name}`, control.min, control.max, control.default);
      }
    }
  } catch (error) {
    // A definition its types refuse, such as one without `state`.
    problems.push(String(error));
  }
  return problems;
}

async function load() {
  version += 1;
  sdk.host.tools.clear();
  // New code starts its memory again, and has failed in nothing yet.
  memories.clear();
  failed.clear();
  /** The file each tool that holds comes from. */
  const files = new Map<string, string>();
  const errors: Array<{ file: string; message: string }> = [];
  for (const file of readdirSync(folder).sort()) {
    if (!/\.tsx?$/.test(file) || file === "sdk.ts" || file.endsWith(".d.ts")) {
      continue;
    }
    const before = new Set(sdk.host.tools.keys());
    try {
      // A new query is a new module, so the file is read again.
      await import(`${join(folder, file)}?v=${version}`);
    } catch (error) {
      // The tools it defined before it failed are its tools too.
      errors.push({ file, message: String(error) });
    }
    for (const [name, spec] of sdk.host.tools) {
      if (before.has(name)) {
        continue;
      }
      const problems = problemsOf(spec);
      errors.push(...problems.map((problem) => ({ file, message: `tool ${name}: ${problem}` })));
      if (problems.length === 0) {
        files.set(name, file);
      }
    }
  }
  // A function of the wrong type is called anyway, so it fails as a problem of its tool.
  const tools = [...sdk.host.tools.values()]
    .filter((spec) => files.has(spec.name))
    .map((spec) => ({
      name: spec.name,
      file: files.get(spec.name),
      title: spec.title,
      when: spec.when,
      doc: spec.doc,
      kind: spec.kind ?? "effect",
      fields: spec.state,
      controls: spec.controls ?? {},
      tick: spec.tick !== undefined,
      page: spec.page !== undefined,
      keys: spec.onKey !== undefined,
      midi: spec.onMidi !== undefined,
    }));
  send({ type: "loaded", tools, errors });
}

function specOf(tool: string): Spec {
  const spec = sdk.host.tools.get(tool);
  if (!spec) {
    throw new Error(`no tool ${tool} is loaded`);
  }
  return spec;
}

/** The sound graph of a tool for these choices. */
function sound(tool: string, choices: Record<string, string | number>): Graph {
  const spec = specOf(tool);
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
  return sdk.graphToJson(() => spec.sound(fields as never));
}

function flatten(child: Child, handlers: Handler[], into: Sent[]) {
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

/** A node as the runtime gets it, with its handlers kept for its card. */
function serialize(node: Node, handlers: Handler[]): Sent {
  const keep = (handler: Handler | undefined) => handler && handlers.push(handler) - 1;
  if (node.type === "canvas") {
    const { onPress, onDrag, ...rest } = node;
    return { ...rest, onPress: keep(onPress), onDrag: keep(onDrag) };
  }
  if (node.type !== "div") {
    return node;
  }
  const children: Sent[] = [];
  flatten(node.children, handlers, children);
  return { type: "div", style: node.style, onClick: keep(node.onClick), children };
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
function players(instance: string): Performer<Controls> {
  return {
    set(control, value) {
      send({ type: "control", instance, name: control, value });
    },
    fire(control, { at } = {}) {
      checkAt(at);
      send({ type: "control", instance, name: control, at });
    },
    play(pitch, { seconds = 0.25, hold = false, velocity = 0.8, at } = {}) {
      checkAt(at);
      // A held note has no length: it ends at its release.
      send({ type: "note", instance, pitch: keyOf(pitch), velocity, seconds: hold ? undefined : seconds, at });
    },
    release(pitch, { at } = {}) {
      checkAt(at);
      send({ type: "release", instance, pitch: keyOf(pitch), at });
    },
  };
}

/** The MIDI key of a pitch. */
function keyOf(pitch: number): number {
  return Math.max(0, Math.min(127, Math.round(pitch)));
}

/**
 * The last record sent of each instance, and how many were sent. The runtime may ask again
 * before an edit reaches it: code that started from the record it sends would undo that edit.
 */
const sent = new Map<string, { edits: number; state: State }>();

/** What changes the record of an instance from `state`, each change after the one before. */
function updater(instance: string, state: State) {
  let current = state;
  return (label: string, change: (state: State) => void) => {
    const next = structuredClone(current);
    change(next);
    current = next;
    sent.set(instance, { edits: (sent.get(instance)?.edits ?? 0) + 1, state: next });
    send({ type: "edit", instance, label, state: next });
  };
}

/** The record of an instance as its code last left it: the runtime's once that has every edit sent. */
function latest({ instance, state, edits }: Looped): State {
  const last = sent.get(instance);
  return last && last.edits > edits ? last.state : state;
}

/** Draws a card and sends its tree, or why there is none: the window waits for one or the other. */
function render(request: Render) {
  const { card, instance, tool, state, watches, page } = request;
  drawn.set(card, request);
  try {
    // Such as a tool whose file does not load since a save.
    const spec = specOf(tool);
    const kept: Handler[] = [];
    const node = drawerOf(spec, page !== null)({
      state,
      watches,
      ...(page && { size: page }),
      memory: memoryOf(instance, spec),
      update: updater(instance, state),
      ...players(instance),
    });
    const tree = serialize(node, kept);
    treeVersion += 1;
    const trees = handlers.get(card) ?? new Map<number, Handler[]>();
    trees.set(treeVersion, kept);
    if (trees.size > KEPT_TREES) {
      const [oldest] = trees.keys();
      trees.delete(oldest);
    }
    handlers.set(card, trees);
    send({ type: "tree", card, tree, version: treeVersion });
  } catch (error) {
    send({ type: "tree", card, error: String(error) });
  }
}

/** Answers a question of the runtime with what `make` gives, or why it failed. */
function answer(id: number, make: () => unknown) {
  try {
    send({ type: "answer", id, value: make() });
  } catch (error) {
    send({ type: "answer", id, error: String(error) });
  }
}

/** The tree of a tool's card or page at its defaults, after one tick: a check without a window. */
function draw(tool: string, page: boolean): Sent {
  const spec = specOf(tool);
  const memory = spec.memory ? spec.memory() : {};
  const quiet = { set() {}, fire() {}, play() {}, release() {} };
  try {
    spec.tick?.({ state: {}, watches: {}, memory, dt: 1 / 30, time: 0, ...quiet });
  } catch (error) {
    throw new Error(`its tick fails: ${error}`);
  }
  const node = drawerOf(spec, page)({
    state: {},
    watches: {},
    ...(page && { size: CHECKED_PAGE }),
    memory,
    update() {},
    ...quiet,
  });
  return serialize(node, []);
}

/**
 * Runs the code of each instance that `call` picks, with what a handler gets, then its cards
 * draw again. `what` names that code in a problem.
 */
function run(
  time: number,
  instances: Looped[],
  what: string,
  call: (spec: Spec, tool: Heard<State, Controls, unknown>) => void,
) {
  now = time;
  for (const looped of instances) {
    const { instance, tool, watches } = looped;
    const spec = sdk.host.tools.get(tool);
    if (!spec) {
      continue;
    }
    const state = latest(looped);
    try {
      call(spec, { state, watches, memory: memoryOf(instance, spec), time, update: updater(instance, state), ...players(instance) });
    } catch (error) {
      fail(tool, `its ${what} failed: ${error}`);
      continue;
    }
    for (const last of drawn.values()) {
      if (last.instance === instance) {
        render({ ...last, state, watches });
      }
    }
  }
}

/** The load that runs, so that a request waits for its tools instead of seeing half of them. */
let loading = load();
let reload: ReturnType<typeof setTimeout> | undefined;
watch(folder, (_, file) => {
  if (file === "sdk.ts" || file === "tsconfig.json") {
    return;
  }
  // An editor saves in several writes.
  clearTimeout(reload);
  reload = setTimeout(() => {
    loading = loading.then(load);
  }, 50);
});

for await (const line of console) {
  // Bun gives the end of the input after the last newline as one more, empty, line.
  if (line === "") {
    continue;
  }
  await loading;
  const request = JSON.parse(line) as Request;
  switch (request.type) {
    case "sound":
      answer(request.id, () => sound(request.tool, request.choices));
      break;
    case "draw":
      answer(request.id, () => draw(request.tool, request.page));
      break;
    case "render":
      render(request);
      break;
    case "event": {
      const last = drawn.get(request.card);
      try {
        handlers.get(request.card)?.get(request.version)?.[request.handler]?.(...(request.at ?? [0, 0]));
      } catch (error) {
        if (last) {
          fail(last.tool, `a click on its card failed: ${error}`);
        }
      }
      // It may have changed the memory.
      if (last) {
        render(last);
      }
      break;
    }
    case "frame":
      // A tick does not change the record: that is the composer's.
      run(request.time, request.instances, "tick", (spec, { update: _, ...tool }) =>
        spec.tick?.({ ...tool, dt: request.dt }),
      );
      send({ type: "framed" });
      break;
    case "key": {
      const key = { key: request.key, down: request.down };
      run(request.time, [request.instance], "onKey", (spec, tool) => spec.onKey?.(tool, key));
      break;
    }
    case "midi":
      run(request.time, request.instances, "onMidi", (spec, tool) => {
        for (const message of request.messages) {
          spec.onMidi?.(tool, message);
        }
      });
      break;
    case "drop":
      handlers.delete(request.card);
      drawn.delete(request.card);
      break;
  }
}
// The runtime closed stdin: it quit.
process.exit(0);
