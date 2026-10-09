// Runs the project's own tools, in Bun, with the `extensions` folder of the project as its
// working folder. One JSON message per line: the runtime asks on stdin, this answers on
// stdout. What a tool logs goes to stderr, so it cannot break a message.

import { readdirSync, watch } from "node:fs";
import { join } from "node:path";
import type { Child, Controls, Fields, Handler as Heard, Midi, Node, StateOf, ToolSpec } from "./sdk";

const folder = process.cwd();
const sdk: typeof import("./sdk") = await import(join(folder, "sdk.ts"));

for (const name of ["log", "info", "debug", "warn"] as const) {
  console[name] = (...values: unknown[]) => console.error(...values);
}

type State = StateOf<Fields>;
type Watches = Record<string, number>;
/** An instance as it is now, for its control loop or what it hears. */
type Looped = { instance: string; tool: string; state: State; watches: Watches };

type Request =
  | { type: "sound"; id: number; tool: string; choices: Record<string, string | number> }
  | { type: "draw"; id: number; tool: string; page: boolean }
  | { type: "render"; card: number; instance: string; tool: string; state: State; watches: Watches; page: boolean }
  | { type: "event"; card: number; version: number; handler: number; x?: number; y?: number }
  | { type: "frame"; dt: number; time: number; instances: Looped[] }
  | { type: "key"; time: number; instance: Looped; key: string; down: boolean }
  | { type: "midi"; time: number; instances: Looped[]; messages: Midi[] }
  | { type: "drop"; card: number };

/** What the runtime gets of a node: a handler is its index. */
type Sent =
  | { type: "div"; style?: unknown; onClick?: number; children: Sent[] }
  | { type: "text"; text: string }
  | { type: "canvas"; width: number; height: number; shapes: unknown[]; background?: string; onPress?: number; onDrag?: number }
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
const drawn = new Map<number, Extract<Request, { type: "render" }>>();
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

const HUM_NAME = /^[a-z][a-z0-9_]*$/;

/**
 * What is wrong with the definition of a tool, so the agent that wrote it can fix it: what the
 * runtime cannot see or does not check, its functions and its ranges. The runtime checks the
 * name and the kinds.
 */
function problemsOf(spec: ToolSpec<Fields, Controls, unknown>): string[] {
  const problems: string[] = [];
  for (const key of ["tick", "card", "page", "memory", "onKey", "onMidi"] as const) {
    if (spec[key] !== undefined && typeof spec[key] !== "function") {
      problems.push(`${key}: give a function`);
    }
  }
  for (const key of ["title", "when", "doc"] as const) {
    if (typeof spec[key] !== "string" || spec[key].trim() === "") {
      problems.push(`${key}: write it, it is what the composer and agents read`);
    }
  }
  if (typeof spec.sound !== "function") {
    problems.push("sound: give a function that returns a Signal");
  }
  if (typeof spec.state !== "object" || spec.state === null) {
    problems.push("state: give the fields of its record, {} for none");
  }
  const range = (at: string, min: number, max: number, value: number) => {
    if (!(min < max)) problems.push(`${at}: min must be below max`);
    if (!(value >= min && value <= max)) {
      problems.push(`${at}: default ${value} is outside [${min}, ${max}]`);
    }
  };
  const names = [...Object.keys(spec.state ?? {}), ...Object.keys(spec.controls ?? {})];
  for (const name of names) {
    if (!HUM_NAME.test(name)) {
      problems.push(`${name}: a field or control name is lowercase letters, digits and _, starting with a letter`);
    }
    if (names.indexOf(name) !== names.lastIndexOf(name)) {
      problems.push(`${name}: a field and a control cannot share a name`);
    }
  }
  for (const [name, field] of Object.entries(spec.state ?? {})) {
    const at = `state.${name}`;
    if (field.kind === "knob") {
      range(at, field.min, field.max, field.default);
    } else if (field.kind === "pattern") {
      range(at, field.min, field.max, field.default);
      if (!(Number.isInteger(field.length) && field.length >= 1 && field.length <= 1024)) {
        problems.push(`${at}: a pattern is 1 to 1024 long`);
      }
    } else if (field.kind === "choice") {
      if (field.options.length === 0) problems.push(`${at}: give at least one option`);
      if (!field.options.includes(field.default)) {
        problems.push(`${at}: default ${JSON.stringify(field.default)} is not one of the options`);
      }
    }
  }
  for (const [name, control] of Object.entries(spec.controls ?? {})) {
    if (control.kind === "live") {
      range(`controls.${name}`, control.min, control.max, control.default);
    }
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
      for (const problem of problems) {
        errors.push({ file, message: `tool ${name}: ${problem}` });
      }
      if (problems.length === 0) {
        files.set(name, file);
      }
    }
  }
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
      tick: typeof spec.tick === "function",
      page: typeof spec.page === "function",
      keys: typeof spec.onKey === "function",
      midi: typeof spec.onMidi === "function",
    }));
  send({ type: "loaded", tools, errors });
}

/**
 * The Hum of a tool for these choices: a line per field and control, which says what the
 * record and the card give the code, then its code.
 */
function sound(tool: string, choices: Record<string, string | number>): string[] {
  const spec = sdk.host.tools.get(tool);
  if (!spec) {
    throw new Error(`no tool ${tool} is loaded`);
  }
  const fields: Record<string, unknown> = {};
  const lines: string[] = [];
  for (const [name, field] of Object.entries(spec.state)) {
    if (field.kind === "choice") {
      fields[name] = choices[name] ?? field.default;
      continue;
    }
    const list = field.kind === "pattern" || field.kind === "sample";
    fields[name] = list ? new sdk.Table(name) : new sdk.Param(name);
    if (field.kind === "sample") {
      lines.push(`sample ${name}`);
    } else if (field.kind === "knob") {
      lines.push(`param ${name} = ${field.default} [${field.min}, ${field.max}]`);
    } else if (field.kind === "toggle") {
      lines.push(`param ${name} = ${field.default ? 1 : 0} [0, 1]`);
    } else {
      lines.push(`param ${name}[${field.length}] = ${field.default} [${field.min}, ${field.max}]`);
    }
  }
  for (const [name, control] of Object.entries(spec.controls ?? {})) {
    fields[name] = new sdk.Param(name);
    lines.push(
      control.kind === "live"
        ? `live ${name} = ${control.default} [${control.min}, ${control.max}]`
        : `trigger ${name}`,
    );
  }
  const code = sdk.graphToHum(() => {
    const sound = spec.sound(fields as never);
    const stereo = typeof sound === "object" && "left" in sound && "right" in sound;
    if (!(sound instanceof sdk.Signal) && typeof sound !== "number" && !stereo) {
      throw new Error("sound must return a Signal, or { left, right }, made with the functions of the SDK");
    }
    return sound;
  });
  return [...lines, ...code];
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
  } else if (typeof child === "object" && "type" in child) {
    into.push(serialize(child, handlers));
  } else {
    throw new Error(`a child is a ${typeof child}, not a node or text`);
  }
}

/** The index of a handler, kept for its card. */
function keep(handler: Handler | undefined, handlers: Handler[]): number | undefined {
  return handler && handlers.push(handler) - 1;
}

function serialize(node: Node, handlers: Handler[]): Sent {
  if (node.type === "canvas") {
    const { onPress, onDrag, ...rest } = node;
    return { ...rest, onPress: keep(onPress, handlers), onDrag: keep(onDrag, handlers) };
  }
  if (node.type !== "div") {
    return node;
  }
  const children: Sent[] = [];
  flatten(node.children, handlers, children);
  return { type: "div", style: node.style, onClick: keep(node.onClick, handlers), children };
}

/** The memory of an instance, made when it is first needed. */
function memoryOf(instance: string, spec: ToolSpec<Fields, Controls, unknown>): unknown {
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
function players(instance: string) {
  return {
    set(control: string, value: number) {
      send({ type: "control", instance, name: control, value });
    },
    fire(control: string, { at }: { at?: number } = {}) {
      checkAt(at);
      send({ type: "control", instance, name: control, at });
    },
    play(
      pitch: number,
      { seconds = 0.25, hold = false, velocity = 0.8, at }: { seconds?: number; hold?: boolean; velocity?: number; at?: number } = {},
    ) {
      checkAt(at);
      // A held note has no length: it ends at its release.
      send({ type: "note", instance, pitch: keyOf(pitch), velocity, seconds: hold ? undefined : seconds, at });
    },
    release(pitch: number, { at }: { at?: number } = {}) {
      checkAt(at);
      send({ type: "release", instance, pitch: keyOf(pitch), at });
    },
  };
}

/** The MIDI key of a pitch. */
function keyOf(pitch: number): number {
  return Math.max(0, Math.min(127, Math.round(pitch)));
}

/** What changes the record of an instance from `state`, each change after the one before. */
function updater(instance: string, state: State) {
  let current = state;
  return (label: string, change: (state: State) => void) => {
    const next = structuredClone(current);
    change(next);
    current = next;
    send({ type: "edit", instance, label, state: next });
  };
}

/** Draws a card and sends its tree, or why there is none: the window waits for one or the other. */
function render(request: Extract<Request, { type: "render" }>) {
  const { card, instance, tool, state, watches, page } = request;
  drawn.set(card, request);
  const spec = sdk.host.tools.get(tool);
  // Such as a tool whose file does not load since a save.
  if (!spec) {
    send({ type: "tree", card, error: `no tool ${tool} is loaded` });
    return;
  }
  const draw = page ? spec.page : (spec.card ?? sdk.defaultCard(spec.state, spec.controls));
  if (!draw) {
    send({ type: "tree", card, error: `${tool} has no page` });
    return;
  }
  try {
    const kept: Handler[] = [];
    const node = (draw as (card: unknown) => Node)({
      state,
      watches,
      memory: memoryOf(instance, spec),
      update: updater(instance, state),
      ...players(instance),
    });
    const tree = serialize(node, kept);
    treeVersion += 1;
    const trees = handlers.get(card) ?? new Map<number, Handler[]>();
    trees.set(treeVersion, kept);
    for (const old of trees.keys()) {
      if (trees.size <= KEPT_TREES) {
        break;
      }
      trees.delete(old);
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
  const spec = sdk.host.tools.get(tool);
  if (!spec) {
    throw new Error(`no tool ${tool} is loaded`);
  }
  const memory = spec.memory ? spec.memory() : {};
  const quiet = { set() {}, fire() {}, play() {}, release() {} };
  if (spec.tick) {
    try {
      spec.tick({ state: {}, watches: {}, memory, dt: 1 / 30, time: 0, ...quiet });
    } catch (error) {
      throw new Error(`its tick fails: ${error}`);
    }
  }
  const make = page ? spec.page : (spec.card ?? sdk.defaultCard(spec.state, spec.controls));
  if (!make) {
    throw new Error("it has no page");
  }
  const node = (make as (card: unknown) => Node)({
    state: {},
    watches: {},
    memory,
    update() {},
    ...quiet,
  });
  return serialize(node, []);
}

/** One step of the control loop of each instance, then its cards draw again. */
function frame(request: Extract<Request, { type: "frame" }>) {
  now = request.time;
  for (const { instance, tool, state, watches } of request.instances) {
    const spec = sdk.host.tools.get(tool);
    if (!spec?.tick) {
      continue;
    }
    try {
      spec.tick({ state, watches, memory: memoryOf(instance, spec), dt: request.dt, time: request.time, ...players(instance) });
    } catch (error) {
      fail(tool, `its tick failed: ${error}`);
      continue;
    }
    redraw(instance, state, watches);
  }
  send({ type: "framed" });
}

/** Draws the cards of an instance again, after its code ran. */
function redraw(instance: string, state: State, watches: Watches) {
  for (const last of drawn.values()) {
    if (last.instance === instance) {
      render({ ...last, state, watches });
    }
  }
}

/**
 * Runs `onKey` or `onMidi`, as `run` says, for each instance that hears it, with what a tick
 * gets and `update`; then its cards draw again.
 */
function hear(
  time: number,
  instances: Looped[],
  what: string,
  run: (spec: ToolSpec<Fields, Controls, unknown>, tool: Heard<State, Controls, unknown>) => void,
) {
  now = time;
  for (const { instance, tool, state, watches } of instances) {
    const spec = sdk.host.tools.get(tool);
    if (!spec) {
      continue;
    }
    try {
      run(spec, { state, watches, memory: memoryOf(instance, spec), time, update: updater(instance, state), ...players(instance) });
    } catch (error) {
      fail(tool, `its ${what} failed: ${error}`);
      continue;
    }
    redraw(instance, state, watches);
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
      try {
        handlers.get(request.card)?.get(request.version)?.[request.handler]?.(request.x ?? 0, request.y ?? 0);
      } catch (error) {
        const tool = drawn.get(request.card)?.tool;
        if (tool) {
          fail(tool, `a click on its card failed: ${error}`);
        }
      }
      // It may have changed the memory.
      const last = drawn.get(request.card);
      if (last) {
        render(last);
      }
      break;
    }
    case "frame":
      frame(request);
      break;
    case "key": {
      const key = { key: request.key, down: request.down };
      hear(request.time, [request.instance], "onKey", (spec, tool) => spec.onKey?.(tool, key));
      break;
    }
    case "midi":
      hear(request.time, request.instances, "onMidi", (spec, tool) => {
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
