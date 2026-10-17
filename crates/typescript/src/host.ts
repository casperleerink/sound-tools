// Runs the project's own tools, in Bun, with the `extensions` folder of the project as its
// working folder. It loads the files once: a change of the code starts a new Bun. One JSON
// message per line: the runtime tells on stdin, this says on stdout. What a tool logs goes to
// stderr, so it cannot break a message.

import { writeSync } from "node:fs";
import { join } from "node:path";
import type { Child, Controls, Fields, Graph, Handler as Heard, Midi, Node, Performer, StateOf, ToolSpec, Transport } from "./sdk";

const folder = process.cwd();
/** The files of tools to load, in order: the runtime leaves out those whose code hung or stopped Bun. */
const files = process.argv.slice(2);
const sdk: typeof import("./sdk") = await import(join(folder, "sdk.ts"));

for (const name of ["log", "info", "debug", "warn"] as const) {
  console[name] = (...values: unknown[]) => console.error(...values);
}

type Spec = ToolSpec<Fields, Controls, unknown>;
type State = StateOf<Fields>;
type Choices = Record<string, string | number>;
type Watches = Record<string, number>;
type Size = { width: number; height: number };
/** The room of a page in the check without a window: the default window's. */
const CHECKED_PAGE: Size = { width: 1422, height: 824 };
/** An instance as it is now, for its control loop or what it hears: `edits` counts the edits from here its `state` has. */
type Looped = { instance: string; tool: string; state: State; watches: Watches; edits: number };

type Render = { type: "render"; card: number; instance: string; tool: string; state: State; watches: Watches; page: Size | null; transport: Transport };
type Request =
  | Render
  /** A click, or a press or a drag on a canvas `at` across and down. */
  | { type: "event"; card: number; version: number; handler: number; at: [number, number] | null }
  | { type: "frame"; dt: number; time: number; instances: Looped[] }
  | { type: "key"; time: number; instance: Looped; key: string; down: boolean }
  | { type: "midi"; time: number; instances: Looped[]; messages: Midi[] }
  | { type: "drop"; card: number }
  | { type: "behaviour"; question: number; tool: string; state: State; children: unknown[] };

type Canvas = Extract<Node, { type: "canvas" }>;
/** What the runtime gets of a node: a handler is its index. */
type Sent =
  | { type: "div"; style?: unknown; onClick?: number; children: Sent[] }
  | { type: "text"; text: string }
  | (Omit<Canvas, "onPress" | "onDrag"> & { onPress?: number; onDrag?: number })
  | Exclude<Node, { type: "div" | "canvas" }>;

/** A click, which hears nothing, or a press or a drag on a canvas, which hears where. */
type Handler = (x: number, y: number) => void;

/** Says something to the runtime, at once: before code that may hang runs, and in order. */
function send(message: object) {
  writeSync(1, JSON.stringify(message) + "\n");
}

/** Tells the runtime whose code runs now, so it knows what to blame when it hangs. */
function runs(tool: string, what: string) {
  send({ type: "runs", tool, what });
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
/** What each card was last drawn from, so it draws again after a tick or a click. */
const drawn = new Map<number, Render>();
/** What the control loop and the cards of each instance keep. */
const memories = new Map<string, unknown>();
/** The tools whose tick or a click on whose card failed. */
const failed = new Set<string>();

/** Tells the runtime the first failure of a tool: it lists it as a problem. */
function fail(tool: string, message: string) {
  if (!failed.has(tool)) {
    failed.add(tool);
    console.error(`${tool}: ${message}`);
    send({ type: "failed", tool, message });
  }
}

const NAME = /^[a-z][a-z0-9_]*$/;
/** The most combinations of choices a tool has: the sound of each is made when it loads. */
const MAX_SOUNDS = 256;

/**
 * What is wrong with the definition of a tool that its types do not say, so the agent that
 * wrote it can fix it. The runtime checks the name, the kinds and the ranges.
 */
function problemsOf(spec: Spec): string[] {
  const problems: string[] = [];
  try {
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
    const sounds = choiceFields(spec).reduce((count, [, field]) => count * field.options.length, 1);
    if (sounds > MAX_SOUNDS) {
      problems.push(`its choices make ${sounds} sounds, at most ${MAX_SOUNDS}: make some of them knobs or toggles`);
    }
    if (spec.behaviour && (spec.kind ?? "effect") !== "effect") {
      problems.push("kind: an owner's sound hears its children through `input`, so leave `kind` out");
    }
  } catch (error) {
    // A definition its types refuse, such as one without `state`.
    problems.push(String(error));
  }
  return problems;
}

/** Loads the files, each after it says which, so a file that hangs is known. */
async function load() {
  const tools: object[] = [];
  const errors: Array<{ file: string; message: string }> = [];
  for (const file of files) {
    send({ type: "loading", file });
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
      const problems = problemsOf(spec);
      errors.push(...problems.map((problem) => ({ file, message: `tool ${name}: ${problem}` })));
      if (problems.length === 0) {
        tools.push(described(spec, file));
      }
    }
  }
  send({ type: "loaded", tools, errors });
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
      midi: spec.onMidi !== undefined,
      behaviour: spec.behaviour !== undefined,
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

const DATA = new Set(["notes", "text", "number", "list"]);

/** The sound graph of a tool for these choices. */
function sound(spec: Spec, choices: Choices): Graph {
  const fields: Record<string, unknown> = {};
  for (const [name, field] of Object.entries(spec.state)) {
    if (DATA.has(field.kind)) {
      continue;
    }
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

/** Answers what the behaviour of an owner declares, or why it does not. */
function declare({ question, tool, state, children }: Extract<Request, { type: "behaviour" }>) {
  const declaration = made(() => {
    const spec = specOf(tool);
    if (!spec.behaviour) {
      throw new Error(`${tool} has no behaviour`);
    }
    runs(tool, "behaviour");
    // What the runtime reads: a function or a class in it would be lost on the way.
    return JSON.parse(JSON.stringify(spec.behaviour({ state, children } as never)));
  });
  send({ type: "declared", question, declaration });
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
    launch(player, pattern, { quantize = "bar" } = {}) {
      if (pattern !== null && !(Number.isInteger(pattern) && pattern >= 0)) {
        throw new RangeError(`launch takes the index of a pattern, from 0, or null to stop, not ${pattern}`);
      }
      if (!QUANTIZE.includes(quantize)) {
        throw new RangeError(`quantize is "bar", "beat" or "now", not ${JSON.stringify(quantize)}`);
      }
      send({ type: "launch", instance, player, pattern, quantize });
    },
  };
}

const QUANTIZE: readonly unknown[] = ["bar", "beat", "now"];

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
  const { card, instance, tool, state, watches, page, transport } = request;
  drawn.set(card, request);
  const tree = made(() => {
    // Such as a tool whose file this Bun runs without.
    const spec = specOf(tool);
    const kept: Handler[] = [];
    runs(tool, page ? "page" : "card");
    const node = drawerOf(spec, page !== null)({
      state,
      watches,
      ...(page && { size: page }),
      transport,
      memory: memoryOf(instance, spec),
      update: updater(instance, state),
      ...players(instance),
    });
    const drawnTree = serialize(node, kept);
    treeVersion += 1;
    const trees = handlers.get(card) ?? new Map<number, Handler[]>();
    trees.set(treeVersion, kept);
    if (trees.size > KEPT_TREES) {
      const [oldest] = trees.keys();
      trees.delete(oldest);
    }
    handlers.set(card, trees);
    return [treeVersion, drawnTree];
  });
  send({ type: "tree", card, tree });
}

/** The tree of a tool's card or page at its defaults, after one tick: a check without a window. */
function draw(spec: Spec, page: boolean): Sent {
  const memory = spec.memory ? spec.memory() : {};
  const quiet = { set() {}, fire() {}, play() {}, release() {}, launch() {} };
  try {
    spec.tick?.({ state: {}, watches: {}, memory, dt: 1 / 30, time: 0, ...quiet });
  } catch (error) {
    throw new Error(`its tick fails: ${error}`);
  }
  const node = drawerOf(spec, page)({
    state: {},
    watches: {},
    ...(page && { size: CHECKED_PAGE }),
    transport: { playing: false, tick: 0, bar: 1, beat: 1, bpm: 120 },
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
      runs(tool, what);
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

function handle(request: Request) {
  switch (request.type) {
    case "render":
      render(request);
      break;
    case "event": {
      const last = drawn.get(request.card);
      try {
        if (last) {
          runs(last.tool, "click handler");
        }
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
    send({ type: "done" });
  }
}
// The runtime closed stdin: it quit.
process.exit(0);
