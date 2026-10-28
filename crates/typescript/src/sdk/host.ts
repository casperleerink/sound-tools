// For the host.

import { Fields, Inputs } from "./fields";
import { PLAIN_KEYS, Press } from "./live";
import { CardRoot, Child, forget, insert, Made, makeNode, materialize, nodes, Op, send, UiNode } from "./nodes";
import { adopted, batch, isRead, listener, Owner, readFields, reconcile, root, runIn, settingUp, setup, store } from "./reactivity";
import { Context, Key, ToolSpec } from "./tools";

/** What the components of a card read while it is made. */
export type Drawing = { fields: Fields } & Pick<Context<Record<string, unknown>>, "state" | "children" | "update" | "openMenu">;

/** Makes the nodes of a card: `draw` runs once. The window gets them with `takeOps`. */
function mount(id: number, draw: () => Child, drawn: Drawing): CardRoot {
  const card: CardRoot = { id, ops: [], changes: new Map(), drags: new Map(), drawn, dispose: () => {} };
  root((dispose) => {
    let top: Made | undefined;
    card.dispose = () => {
      dispose();
      if (top) forget(top);
    };
    try {
      const made: UiNode[] = [];
      materialize(setup(draw), made, "div");
      const [only] = made as Made[];
      top = only;
      if (made.length !== 1 || !only || only.family === "marker" || only.family === "text") {
        top = makeNode("div");
        send(top);
        for (const child of made as Made[]) {
          (top.children ??= new Set()).add(child);
          insert(top.id, child, null);
        }
      }
      card.ops.push(["root", (top as Made).id]);
    } catch (error) {
      card.dispose();
      throw error;
    }
  }, card);
  return card;
}

/** What changed on `card` since the last time, as ops for the window, or why it failed. */
function takeOps(card: CardRoot): { ops: Op[] } | { error: string } {
  if (card.error !== undefined) return { error: card.error };
  const ops = card.ops;
  for (const [made, props] of card.changes) ops.push(["set", made.id, made.family, props]);
  card.ops = [];
  card.changes.clear();
  return { ops };
}

/**
 * The `onRelease` of each press, by node and button, with the scope of its node's handlers: a
 * node its press took away still hears its release, and what that starts goes at once.
 */
const releases = new Map<string, { release: (press: Press) => void; scope: Owner }>();

/** The pointer, the end of a drag, a drop, a field's text, a card's icon or a control's new value, on the node `id`. */
function nodeEvent(id: number, kind: "press" | "release" | "click" | "hover" | "end" | "commit" | "cancel" | "drop" | "power" | "close" | "title" | "sidechain" | "change" | "key", payload: never) {
  if (kind === "release") {
    const pressed = `${id} ${(payload as Press).button}`;
    const held = releases.get(pressed);
    releases.delete(pressed);
    return held && runIn(held.scope, () => held.release(payload));
  }
  const made = nodes.get(id);
  if (!made) return;
  const scope = (made.scope ??= { owner: null, card: made.card, disposed: false });
  if (kind === "press" && made.onRelease) releases.set(`${id} ${(payload as Press).button}`, { release: made.onRelease, scope });
  return runIn(scope, () => handle(made, kind, payload));
}

function handle(made: Made, kind: Exclude<Parameters<typeof nodeEvent>[1], "release">, payload: never) {
  switch (kind) {
    case "commit":
      return made.onCommit?.((payload as { text: string }).text);
    case "cancel":
      return made.onCancel?.();
    case "change":
      return made.onChange?.(payload);
    case "hover":
      return made.onHover?.(payload as boolean);
    case "power":
      return made.onPower?.();
    case "close":
      return made.onClose?.();
    case "title":
      return made.onTitle?.();
    case "key":
      return made.onKey?.(payload as Key);
    case "sidechain": {
      const { select, option } = payload as { select: number; option: number };
      return made.onSidechain?.(select, option);
    }
    default: {
      const handler = { press: made.onPress, click: made.onClick, end: made.drag?.onEnd, drop: made.onDrop }[kind];
      return handler?.(payload);
    }
  }
}

/** What the host reads. Not for tools. */
export const host = {
  /** Whether `name` names a key as `KeyName` does. */
  isKeyName: (name: string) => PLAIN_KEYS.includes(name.replace(/^(cmd-)?(ctrl-)?(alt-)?/, "") as never),
  tools: new Map<string, ToolSpec<Fields, Inputs, unknown>>(),
  /** The call stack of each tool's `tool`, which tells the file that made it. */
  stacks: new Map<string, string>(),
  mount,
  takeOps,
  nodeEvent,
  store,
  reconcile,
  adopted,
  readFields,
  isRead,
  batch,
  /** Called once bindings ran again; the host sends their ops. */
  changed: () => {},
  /** Whether a binding runs now, or a component is made: what reads there is followed. */
  binding: () => listener !== null || settingUp > 0,
};
