// Runs the project's own tools, in Bun, with the `extensions` folder of the project as its
// working folder. One JSON message per line: the runtime asks on stdin, this answers on
// stdout. What a tool logs goes to stderr, so it cannot break a message.

import { readdirSync, watch } from "node:fs";
import { join } from "node:path";
import type { Child, Controls, Fields, Node, StateOf, ToolSpec } from "./sdk";

const folder = process.cwd();
const sdk: typeof import("./sdk") = await import(join(folder, "sdk.ts"));

for (const name of ["log", "info", "debug", "warn"] as const) {
  console[name] = (...values: unknown[]) => console.error(...values);
}

type State = StateOf<Fields>;
type Watches = Record<string, number>;

type Request =
  | { type: "sound"; id: number; tool: string; choices: Record<string, string | number> }
  | { type: "draw"; id: number; tool: string; page: boolean }
  | { type: "render"; card: number; instance: string; tool: string; state: State; watches: Watches; page: boolean }
  | { type: "event"; card: number; handler: number; x?: number; y?: number }
  | { type: "frame"; dt: number; instances: Array<{ instance: string; tool: string; state: State; watches: Watches }> }
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

/** The handlers of the last tree of each card. */
const handlers = new Map<number, Handler[]>();
/** What each card was last drawn from, so it draws again after a tick or a click. */
const drawn = new Map<number, Extract<Request, { type: "render" }>>();
/** What the control loop and the cards of each instance keep. */
const memories = new Map<string, unknown>();
let version = 0;

const NAME = /^[a-z0-9_-]+$/;
const HUM_NAME = /^[a-z_][a-z0-9_]*$/;

/** What is wrong with the definition of a tool, so the agent that wrote it can fix it. */
function problemsOf(spec: ToolSpec<Fields, Controls, unknown>): string[] {
  const problems: string[] = [];
  if (!NAME.test(spec.name ?? "")) {
    problems.push(`name ${JSON.stringify(spec.name)}: use lowercase letters, digits, - and _`);
  }
  for (const key of ["tick", "card", "page", "memory"] as const) {
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
  const kind = spec.kind ?? "effect";
  if (!["effect", "instrument", "source"].includes(kind)) {
    problems.push(`kind ${JSON.stringify(kind)}: use "effect", "instrument" or "source"`);
  }
  if (spec.voices !== undefined && (kind !== "instrument" || !(spec.voices >= 1 && spec.voices <= 8))) {
    problems.push("voices: only an instrument has voices, from 1 to 8");
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
    } else if (field.kind !== "toggle") {
      problems.push(`${at}: make it with knob(), toggle(), choice() or pattern()`);
    }
  }
  for (const [name, control] of Object.entries(spec.controls ?? {})) {
    if (control.kind === "live") {
      range(`controls.${name}`, control.min, control.max, control.default);
    } else if (control.kind !== "trigger") {
      problems.push(`controls.${name}: make it with live() or trigger()`);
    }
  }
  return problems;
}

async function load() {
  version += 1;
  sdk.host.tools.clear();
  // New code starts its memory again.
  memories.clear();
  /** The file each tool comes from. */
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
      files.set(name, file);
      for (const problem of problemsOf(spec)) {
        errors.push({ file, message: `tool ${name}: ${problem}` });
      }
    }
  }
  const tools = [...sdk.host.tools.values()]
    .filter((spec) => problemsOf(spec).length === 0)
    .map((spec) => ({
      name: spec.name,
      file: files.get(spec.name),
      title: spec.title,
      when: spec.when,
      doc: spec.doc,
      kind: spec.kind ?? "effect",
      voices: spec.voices,
      fields: spec.state,
      controls: spec.controls ?? {},
      tick: typeof spec.tick === "function",
      page: typeof spec.page === "function",
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
    fields[name] = field.kind === "pattern" ? new sdk.Table(name) : new sdk.Param(name);
    if (field.kind === "knob") {
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
    if (!(sound instanceof sdk.Signal) && typeof sound !== "number") {
      throw new Error("sound must return a Signal, made with the functions of the SDK");
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
  if (!memories.has(instance)) {
    memories.set(instance, spec.memory ? spec.memory() : {});
  }
  return memories.get(instance);
}

/** What a card, a page and the control loop use to play an instance. */
function players(instance: string) {
  return {
    set(control: string, value: number) {
      send({ type: "control", instance, name: control, value });
    },
    fire(control: string) {
      send({ type: "control", instance, name: control });
    },
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
      update(label: string, change: (state: unknown) => void) {
        const next = structuredClone(state);
        change(next);
        send({ type: "edit", card, label, state: next });
      },
      ...players(instance),
    });
    const tree = serialize(node, kept);
    handlers.set(card, kept);
    send({ type: "tree", card, tree });
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
  const quiet = { set() {}, fire() {} };
  if (spec.tick) {
    try {
      spec.tick({ state: {}, watches: {}, memory, dt: 1 / 30, ...quiet });
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
  for (const { instance, tool, state, watches } of request.instances) {
    const spec = sdk.host.tools.get(tool);
    if (!spec?.tick) {
      continue;
    }
    try {
      spec.tick({ state, watches, memory: memoryOf(instance, spec), dt: request.dt, ...players(instance) });
    } catch (error) {
      console.error(`the tick of ${tool} failed: ${error}`);
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
      try {
        handlers.get(request.card)?.[request.handler]?.(request.x ?? 0, request.y ?? 0);
      } catch (error) {
        console.error(`a click failed: ${error}`);
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
    case "drop":
      handlers.delete(request.card);
      drawn.delete(request.card);
      break;
  }
}
// The runtime closed stdin: it quit.
process.exit(0);
