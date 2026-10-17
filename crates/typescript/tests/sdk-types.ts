// Types an agent's tool must get right. A test of the crate checks this file with `tsc` next to
// the SDK: every `@ts-expect-error` line must be an error, and nothing else.
import { list, number, ownSound, text, tool } from "./sdk";

const rows = list({ name: text(), steps: number({ min: 0, max: 8 }) }, { default: [{ name: "a" }] });

// @ts-expect-error: an item has no field `title`.
list({ name: text() }, { default: [{ title: "x" }] });

tool({
  name: "types",
  title: "Types",
  when: "Never",
  doc: "Types.",
  state: { rows, plain: list(text()) },
  behaviour: ({ state }) => {
    const name: string = state.rows[0]!.name;
    const steps: number = state.rows[0]!.steps;
    const plain: string = state.plain[0]!;
    // @ts-expect-error: an item has no field `title`.
    state.rows[0]!.title;
    const connections = [{ from: { child: name + steps + plain }, to: ownSound }];
    return { connections };
  },
});
