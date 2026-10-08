// Runs the project's own tools and cards, in Bun, with the `extensions` folder of the project
// as its working folder. One JSON message per line: the runtime asks on stdin, this answers on
// stdout. What a tool logs goes to stderr, so it cannot break a message.

import { readdirSync, watch } from "node:fs";
import { join } from "node:path";
import type { Child, Fields, Node, ToolSpec } from "./sdk";

const folder = process.cwd();
const sdk: typeof import("./sdk") = await import(join(folder, "sdk.ts"));

for (const name of ["log", "info", "debug", "warn"] as const) {
  console[name] = (...values: unknown[]) => console.error(...values);
}

type Request =
  | { type: "sound"; id: number; tool: string; choices: Record<string, string | number> }
  | { type: "render"; card: number; tool: string; state: unknown }
  | { type: "event"; card: number; handler: number }
  | { type: "drop"; card: number };

/** What the runtime gets of a node: a click is the index of its handler. */
type Sent =
  | { type: "div"; style?: unknown; onClick?: number; children: Sent[] }
  | { type: "text"; text: string }
  | { type: "knob"; path: string; label: string; min: number; max: number; default: number };

/** With `SOUND_TOOLS_TIMING` set, what happens when, for a measurement. */
const timing = process.env.SOUND_TOOLS_TIMING
  ? (what: string) => console.error(`timing: bun ${what} at ${Date.now()}`)
  : () => {};

function send(message: object) {
  process.stdout.write(JSON.stringify(message) + "\n");
}

/** The click handlers of the last tree of each card. */
const handlers = new Map<number, Array<() => void>>();
/** The file each tool comes from. */
const files = new Map<string, string>();
let version = 0;

const NAME = /^[a-z0-9_-]+$/;
const HUM_NAME = /^[a-z_][a-z0-9_]*$/;

/** What is wrong with the definition of a tool, so the agent that wrote it can fix it. */
function problemsOf(spec: ToolSpec<Fields>): string[] {
  const problems: string[] = [];
  if (!NAME.test(spec.name ?? "")) {
    problems.push(`name ${JSON.stringify(spec.name)}: use lowercase letters, digits, - and _`);
  }
  for (const key of ["title", "when", "doc"] as const) {
    if (typeof spec[key] !== "string" || spec[key].trim() === "") {
      problems.push(`${key}: write it, it is what the composer and agents read`);
    }
  }
  if (typeof spec.sound !== "function") {
    problems.push("sound: give a function that returns hum`...`");
  }
  for (const [name, field] of Object.entries(spec.state ?? {})) {
    const at = `state.${name}`;
    if (!HUM_NAME.test(name)) {
      problems.push(`${at}: a field name is a Hum name: lowercase letters, digits and _`);
    }
    if (field.kind === "knob") {
      if (!(field.min < field.max)) problems.push(`${at}: min must be below max`);
      if (!(field.default >= field.min && field.default <= field.max)) {
        problems.push(`${at}: default ${field.default} is outside [${field.min}, ${field.max}]`);
      }
    } else if (field.kind === "choice") {
      if (field.options.length === 0) problems.push(`${at}: give at least one option`);
      if (!field.options.includes(field.default)) {
        problems.push(`${at}: default ${JSON.stringify(field.default)} is not one of the options`);
      }
    } else if (field.kind !== "toggle") {
      problems.push(`${at}: make it with knob(), toggle() or choice()`);
    }
  }
  return problems;
}

async function load() {
  timing("loads extensions/");
  version += 1;
  sdk.host.tools.clear();
  sdk.host.cards.clear();
  files.clear();
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
      errors.push({ file, message: String(error) });
      continue;
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
      fields: spec.state,
    }));
  const cards = [...sdk.host.cards.keys(), ...tools.map((tool) => tool.name)];
  send({ type: "loaded", tools, cards, errors });
  timing("loaded extensions/");
}

/** The Hum of a tool for these choices: a `param` line per knob and toggle, then its code. */
function sound(tool: string, choices: Record<string, string | number>): string[] {
  const spec = sdk.host.tools.get(tool);
  if (!spec) {
    throw new Error(`no tool ${tool} is loaded`);
  }
  const fields: Record<string, unknown> = {};
  const params: string[] = [];
  for (const [name, field] of Object.entries(spec.state)) {
    if (field.kind === "choice") {
      fields[name] = choices[name] ?? field.default;
    } else {
      fields[name] = new sdk.Param(name);
      params.push(
        field.kind === "knob"
          ? `param ${name} = ${field.default} [${field.min}, ${field.max}]`
          : `param ${name} = ${field.default ? 1 : 0} [0, 1]`,
      );
    }
  }
  const code = spec.sound(fields as never);
  if (!(code instanceof sdk.Hum)) {
    throw new Error("sound must return hum`...`");
  }
  const lines = code.text.split("\n").map((line) => line.trim());
  return [...params, ...lines];
}

function flatten(child: Child, clicks: Array<() => void>, into: Sent[]) {
  if (child === null || child === undefined || typeof child === "boolean") {
    return;
  }
  if (Array.isArray(child)) {
    for (const each of child) {
      flatten(each, clicks, into);
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
    into.push(serialize(child, clicks));
  } else {
    throw new Error(`a child is a ${typeof child}, not a node or text`);
  }
}

function serialize(node: Node, clicks: Array<() => void>): Sent {
  if (node.type !== "div") {
    return node;
  }
  const children: Sent[] = [];
  flatten(node.children, clicks, children);
  const onClick = node.onClick && clicks.push(node.onClick) - 1;
  return { type: "div", style: node.style, onClick, children };
}

function render(card: number, tool: string, state: unknown) {
  const spec = sdk.host.tools.get(tool);
  const draw = spec ? (spec.card ?? sdk.defaultCard(spec.state)) : sdk.host.cards.get(tool);
  if (!draw) {
    return;
  }
  try {
    const clicks: Array<() => void> = [];
    const node = (draw as (card: unknown) => Node)({
      state,
      update(label: string, change: (state: unknown) => void) {
        const next = structuredClone(state);
        change(next);
        send({ type: "edit", card, label, state: next });
      },
    });
    const tree = serialize(node, clicks);
    handlers.set(card, clicks);
    send({ type: "tree", card, tree });
  } catch (error) {
    send({ type: "tree", card, error: String(error) });
  }
}

let reload: ReturnType<typeof setTimeout> | undefined;
watch(folder, (_, file) => {
  if (file === "sdk.ts" || file === "tsconfig.json") {
    return;
  }
  timing(`heard a change of ${file}`);
  // An editor saves in several writes.
  clearTimeout(reload);
  reload = setTimeout(load, 50);
});
await load();

for await (const line of console) {
  const request = JSON.parse(line) as Request;
  timing(`got ${request.type}`);
  switch (request.type) {
    case "sound":
      try {
        send({ type: "sound", id: request.id, code: sound(request.tool, request.choices) });
      } catch (error) {
        send({ type: "sound", id: request.id, error: String(error) });
      }
      break;
    case "render":
      render(request.card, request.tool, request.state);
      break;
    case "event":
      try {
        handlers.get(request.card)?.[request.handler]?.();
      } catch (error) {
        console.error(`a click failed: ${error}`);
      }
      break;
    case "drop":
      handlers.delete(request.card);
      break;
  }
}
// The runtime closed stdin: it quit.
process.exit(0);
