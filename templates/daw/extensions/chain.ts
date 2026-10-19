import { list, text, tool, type Connection, type Declaration } from "./sdk";

export const chain = tool({
  name: "chain",
  title: "Chain",
  when: "You want a sound through any children, in an order a record names, each one on or off",
  doc: "Plays what it hears through the children `order` names, in that order. A child named in `off` is passed by, and a child `order` does not name does not play. With nothing to play through it is silent: a parent leaves it out of its wiring then.",
  state: { order: list(text()), off: list(text()) },
  behaviour: ({ state: { order, off }, children }): Declaration => {
    const playing = order.filter((name) => !off.includes(name) && children.some((child) => child.name === name));
    if (playing.length === 0) return { connections: [] };
    const connections: Connection[] = [{ from: { port: "audio" }, to: { child: playing[0]! } }];
    playing.slice(1).forEach((name, index) => connections.push({ from: { child: playing[index]! }, to: { child: name } }));
    connections.push({ from: { child: playing[playing.length - 1]! }, to: { port: "audio" } });
    return { connections };
  },
});

/** Whether a chain plays through anything, as its behaviour wires it. Its children are read only when it names some. */
export function chainPlays(chain: { readonly state: { readonly order?: readonly string[]; readonly off?: readonly string[] }; readonly children: readonly { name: string }[] }): boolean {
  const { order = [], off = [] } = chain.state;
  return order.some((name) => !off.includes(name) && chain.children.some((child) => child.name === name));
}
