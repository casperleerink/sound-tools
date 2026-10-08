// Runs the cards of a project, in Bun, with the `ui` folder of the project as its working
// folder. One JSON message per line: the runtime asks on stdin, this answers on stdout. What a
// card logs goes to stderr, so it cannot break a message.

import { readdirSync, watch } from "node:fs";
import { join } from "node:path";
import type { Child, Node } from "./sdk";

const folder = process.cwd();
const sdk: typeof import("./sdk") = await import(join(folder, "sdk.ts"));

for (const name of ["log", "info", "debug", "warn"] as const) {
  console[name] = (...values: unknown[]) => console.error(...values);
}

type Request =
  | { type: "render"; card: number; tool: string; state: unknown }
  | { type: "event"; card: number; handler: number }
  | { type: "drop"; card: number };

/** What the runtime gets of a node: a click is the index of its handler. */
type Sent =
  | { type: "div"; style?: unknown; onClick?: number; children: Sent[] }
  | { type: "text"; text: string }
  | { type: "knob"; path: string; label: string; min: number; max: number; default: number };

function send(message: object) {
  process.stdout.write(JSON.stringify(message) + "\n");
}

/** The click handlers of the last tree of each card. */
const handlers = new Map<number, Array<() => void>>();
let version = 0;

async function load() {
  version += 1;
  sdk.host.cards.clear();
  const errors: string[] = [];
  for (const file of readdirSync(folder).sort()) {
    if (!/\.tsx?$/.test(file) || file === "sdk.ts" || file.endsWith(".d.ts")) {
      continue;
    }
    try {
      // A new query is a new module, so the file is read again.
      await import(`${join(folder, file)}?v=${version}`);
    } catch (error) {
      errors.push(`ui/${file}: ${error}`);
    }
  }
  send({ type: "loaded", tools: [...sdk.host.cards.keys()], errors });
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
  const draw = sdk.host.cards.get(tool);
  if (!draw) {
    return;
  }
  try {
    const clicks: Array<() => void> = [];
    const node = draw({
      state,
      update(label, change) {
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
watch(folder, () => {
  // An editor saves in several writes.
  clearTimeout(reload);
  reload = setTimeout(load, 50);
});
await load();

for await (const line of console) {
  const request = JSON.parse(line) as Request;
  switch (request.type) {
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
