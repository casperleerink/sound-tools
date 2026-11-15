// Runs one tool for the window. In, JSON lines: {init: state}, {set: ops}, {press: key},
// {drag: {key, dx, dy}}. Out: {tree: {root, memos, drop}}, {change: ops}, {error}.
import { apply, dragEnd, press, render, type Node, type Op } from "./project/sdk";

const tool: { view: (state: unknown) => Node } = await import(require("node:path").resolve(Bun.argv[2]!));
let state: unknown;
const send = (message: object) => process.stdout.write(JSON.stringify(message) + "\n");
const guard = (run: () => void) => {
  try {
    run();
  } catch (error) {
    send({ error: String(error instanceof Error ? error.stack : error) });
  }
};

const decoder = new TextDecoder();
let buffer = "";
for await (const chunk of Bun.stdin.stream()) {
  buffer += decoder.decode(chunk, { stream: true });
  let dirty = false;
  for (let nl = buffer.indexOf("\n"); nl >= 0; nl = buffer.indexOf("\n")) {
    const message = JSON.parse(buffer.slice(0, nl));
    buffer = buffer.slice(nl + 1);
    let asked: Op[] = [];
    if ("init" in message) [state, dirty] = [message.init, true];
    else if ("set" in message) [state, dirty] = [apply(state, message.set), true];
    else if ("press" in message) guard(() => (asked = press(message.press)));
    else if ("drag" in message) guard(() => (asked = dragEnd(message.drag.key, message.drag)));
    // The tool's own change shows at once; the host keeps it and sends it back only if it differs.
    if (asked.length > 0) [state, dirty] = [(send({ change: asked }), apply(state, asked)), true];
  }
  // Every message that arrived together is one render.
  if (dirty) guard(() => send({ tree: render(tool.view, state) }));
}
