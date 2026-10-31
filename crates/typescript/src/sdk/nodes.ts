// Nodes: what a card makes, which the window keeps by id. A change sends the window only what
// changed: a prop of a node, a node in or out.

import { Shape, shapeNode, ShapeProps } from "./drawing";
import { Drawing } from "./host";
import { Drag, Dropped, PointerEvents } from "./live";
import type { Key } from "./tools";
import { Accessor, dispose, effect, memo, onCleanup, owner, Owner, RAW, root, setup, signal, untrack } from "./reactivity";

declare const nodeBrand: unique symbol;

/** A node the window draws: what JSX makes. */
export interface UiNode {
  readonly [nodeBrand]: true;
}

/** Shows what a function gives, a list, or one of two, in place: see `For` and `Show`. */
export interface Region {
  readonly mount: (into: UiNode[], family: Family) => void;
}

/** Anything a node takes as a child. A function is a binding: what it gives is shown again when what it read changes. */
export type Child = UiNode | Region | ShapeLiteral | string | number | boolean | null | undefined | (() => Child) | Child[];

type Family = "div" | "text" | "space" | "shape" | "input" | "card" | "button" | "icon" | "toggle" | "slider" | "segmented" | "notice" | "overlay" | "marker";
/** What the window gets of a node: a pointer handler is `true`, a drag the number of its kept drag. */
type Wired = Record<string, unknown>;

export interface Made extends PointerEvents {
  readonly [nodeBrand]: true;
  id: number;
  family: Family;
  card: CardRoot;
  props: Wired;
  /** Sent to the window: changes after this go as `set`. */
  sent: boolean;
  onHover?: (over: boolean) => void;
  onDrop?: (drop: Dropped) => void;
  onCommit?: (text: string) => void;
  onCancel?: () => void;
  /** A field's text, or a segmented control's place. */
  onChange?: (value: string | number) => void;
  onPower?: () => void;
  onClose?: () => void;
  onTitle?: () => void;
  onSidechain?: (select: number, option: number) => void;
  onKey?: (key: Key) => void;
  drag?: Drag;
  /** Where its handlers run, made at the first: what they start, such as a timer, goes with the node. */
  scope?: Owner;
  /** Made on the first: a shape has none. */
  children?: Set<Made>;
  /** The region a marker stands for, which puts what it shows before the marker. */
  region?: { parent: number | null };
}

/** What the window gets of a card at once: the ops since the last time. */
export type Op = [string, ...unknown[]];

/** The nodes of one card. */
export interface CardRoot {
  id: number;
  ops: Op[];
  /** Props changed since the last ops, by node: sent as one `set` each. */
  changes: Map<Made, Wired>;
  drags: Map<Drag, number>;
  /** What its components read: its record, its fields and what changes them. */
  drawn: Drawing;
  error?: string;
  dispose: () => void;
}

/** The window drops a press on a node an earlier Bun made, so ids start again with each Bun. */
let nextNode = 1;
export const nodes = new Map<number, Made>();

export function currentCard(): CardRoot {
  const card = owner?.card;
  if (!card) throw new Error("a node is made outside a view");
  return card;
}

export function makeNode(family: Family): Made {
  const made: Made = { id: nextNode++, family, card: currentCard(), props: {}, sent: false } as unknown as Made;
  nodes.set(made.id, made);
  return made;
}

export function send(made: Made) {
  made.card.ops.push(["create", made.id, made.family, made.props]);
  made.sent = true;
}

function setProp(made: Made, name: string, value: unknown) {
  const wired = value === undefined ? null : value;
  // An object, such as a live value, is new each time its binding runs: it goes as it is.
  if (Object.is(made.props[name] ?? null, wired)) return;
  made.props[name] = wired;
  if (!made.sent) return;
  const pending = made.card.changes.get(made) ?? {};
  pending[name] = wired;
  made.card.changes.set(made, pending);
}

export function insert(parent: number, child: Made, before: Made | null) {
  child.card.ops.push(["insert", parent, child.id, before?.id ?? null]);
  if (child.region) child.region.parent = parent;
}

export function forget(made: Made) {
  nodes.delete(made.id);
  if (made.scope) dispose(made.scope);
  made.card.changes.delete(made);
  for (const child of made.children ?? []) forget(child);
}

function remove(made: Made) {
  // A region whose row goes takes its own nodes out too; one its row took already is gone.
  if (!nodes.has(made.id)) return;
  made.card.ops.push(["remove", made.id]);
  forget(made);
}

/** The number of `drag` on its card: a drag every note shares goes to the window once. */
function keptDrag(card: CardRoot, drag: Drag): number {
  let id = card.drags.get(drag);
  if (id === undefined) {
    id = nextNode++;
    card.drags.set(drag, id);
    card.ops.push(["drag", id, { ...drag, onEnd: drag.onEnd !== undefined }]);
  }
  return id;
}

const HANDLERS = new Set(["onPress", "onRelease", "onClick", "onHover", "onDrop", "onCommit", "onCancel", "onPower", "onClose", "onTitle", "onSidechain", "onChange", "onKey"]);

/** A node of `family` with `props`, bound where a prop is a function, and its children in it. */
export function element(family: Family, props: Record<string, unknown>, children: Child[]): UiNode {
  const made = makeNode(family);
  for (const [name, value] of Object.entries(props)) {
    if (name === "children" || value === undefined) continue;
    if (HANDLERS.has(name)) {
      (made as unknown as Record<string, unknown>)[name] = value;
      made.props[name] = true;
    } else if (name === "drag") {
      made.drag = value as Drag;
      made.props.drag = keptDrag(made.card, value as Drag);
    } else if (typeof value === "function") {
      effect(() => setProp(made, name, (value as Accessor<unknown>)()));
    } else {
      made.props[name] = value;
    }
  }
  const placed: UiNode[] = [];
  for (const child of children) materialize(child, placed, family);
  send(made);
  for (const child of placed as Made[]) {
    (made.children ??= new Set()).add(child);
    insert(made.id, child, null);
  }
  return made;
}

/** A shape as a plain object, as a child of a space. */
export type ShapeLiteral = { [Kind in Shape["kind"]]: { kind: Kind } & ShapeProps<Kind> }[Shape["kind"]];

function isNode(value: unknown): value is Made {
  return typeof value === "object" && value !== null && nodes.has((value as Made).id) && (value as Made).card !== undefined;
}

/** The nodes `child` makes, in order, as children of a node of `family`. */
export function materialize(child: Child, into: UiNode[], family: Family) {
  if (child === null || child === undefined || typeof child === "boolean") return;
  if (Array.isArray(child)) {
    for (const each of child) materialize(each, into, family);
  } else if (typeof child === "function") {
    dynamic(child, into, family);
  } else if (typeof child === "string" || typeof child === "number") {
    if (family === "space" || family === "shape") throw new Error(`a Space draws shapes such as Rect and Label, not the text ${JSON.stringify(child)}`);
    const text = makeNode("text");
    text.props.text = String(child);
    send(text);
    into.push(text);
  } else if ("mount" in child && typeof child.mount === "function") {
    child.mount(into, family);
  } else if (isNode(child)) {
    if ((family === "space" || family === "shape") && child.family !== "shape" && child.family !== "marker") {
      throw new Error(`a Space draws shapes such as Rect and Label, not a ${child.family}`);
    }
    if (family !== "space" && family !== "shape" && child.family === "shape") {
      throw new Error("a shape is drawn inside a Space");
    }
    into.push(child);
  } else if ("kind" in child) {
    const { kind, ...props } = child as ShapeLiteral;
    into.push(shapeNode(kind, props, []));
  } else {
    throw new Error("a child is a node, text, a list or a function that gives one: not a node of a list that is gone");
  }
}

/** A marker the nodes of a region go before, so they keep their place among their siblings. */
function marker(): Made {
  const made = makeNode("marker");
  made.region = { parent: null };
  send(made);
  return made;
}

/** Where a region puts nodes now: in its parent before `marker`, or into the list its parent is made from. */
function place(region: Made, nodesToPlace: Made[], before: Made, into: UiNode[] | null) {
  const parent = region.region?.parent;
  for (const made of nodesToPlace) {
    if (into) into.push(made);
    else if (parent !== null && parent !== undefined) insert(parent, made, before);
    const parentNode = parent === null || parent === undefined ? undefined : nodes.get(parent);
    if (parentNode) (parentNode.children ??= new Set()).add(made);
  }
}

/** Takes `made` out of the parent of the region that ends at `end`. */
function takeOut(end: Made, made: Made) {
  const parent = end.region?.parent;
  if (parent !== null && parent !== undefined) nodes.get(parent)?.children?.delete(made);
  remove(made);
}

/** A child that is a function: shown again whole when what it read changes; text changes in place. */
function dynamic(show: Accessor<Child>, into: UiNode[], family: Family) {
  const end = marker();
  let shown: Made[] = [];
  let first: UiNode[] | null = into;
  // What it showed after the first time is in its parent, not in the row around it: it goes
  // when that row does.
  onCleanup(() => shown.forEach((made) => takeOut(end, made)));
  effect(() => {
    const value = show();
    const parent = end.region?.parent;
    const parentNode = parent === null || parent === undefined ? undefined : nodes.get(parent);
    if ((typeof value === "string" || typeof value === "number") && shown.length === 1 && shown[0]?.family === "text") {
      setProp(shown[0], "text", String(value));
      return;
    }
    const next: UiNode[] = [];
    materialize(value, next, family);
    for (const old of shown) {
      if (!next.includes(old)) {
        parentNode?.children?.delete(old);
        remove(old);
      }
    }
    shown = next as Made[];
    place(end, shown, end, first);
    first = null;
  });
  into.push(end);
}

/**
 * A list of `each`, one row per item, kept by its item: a row is made once, moves when its item
 * moves and goes when its item goes; an item that is in the list twice has two rows. Its body
 * runs once, as a component's; `index` follows its place.
 * `<For each={() => state.notes}>{(note, index) => <Rect x={() => note.start} ... />}</For>`
 */
export function For<T>(props: { each: Accessor<readonly T[]>; children: (item: T, index: Accessor<number>) => Child }): Region {
  return {
    mount(into, family) {
      const end = marker();
      type Row = { nodes: Made[]; index: (next: number) => void; dispose: () => void };
      // The rows of each item, in the order it comes in the list.
      let rows = new Map<T, Row[]>();
      let order: Row[] = [];
      let first: UiNode[] | null = into;
      // Rows made after the first are in its parent, not in the row around it: they go when it does.
      onCleanup(() => {
        for (const row of order) {
          row.nodes.forEach((made) => takeOut(end, made));
          row.dispose();
        }
      });
      effect(() => {
        const list = props.each();
        // A store list is followed as a whole, not item by item: a list of thousands costs one read.
        const items = (list as unknown as Record<symbol, unknown>)[RAW] ? (Reflect.ownKeys(list), untrack(() => [...list])) : [...list];
        untrack(() => {
          const parent = end.region?.parent;
          const parentNode = parent === null || parent === undefined ? undefined : nodes.get(parent);
          const left = new Map([...rows].map(([item, each]) => [item, [...each]]));
          const kept = items.map((item) => left.get(item)?.shift());
          for (const row of [...left.values()].flat()) {
            for (const made of row.nodes) {
              parentNode?.children?.delete(made);
              remove(made);
            }
            row.dispose();
          }
          const fresh = new Set<Row>();
          const next = new Map<T, Row[]>();
          const nextOrder = items.map((item, index) => {
            let row = kept[index];
            if (row) {
              row.index(index);
            } else {
              row = root((dispose) => {
                const [readIndex, setIndex] = signal(index);
                const made: UiNode[] = [];
                materialize(setup(() => props.children(item, readIndex)), made, family);
                return { nodes: made as Made[], index: setIndex, dispose };
              });
              fresh.add(row);
            }
            next.set(item, [...(next.get(item) ?? []), row]);
            return row;
          });
          if (first) {
            for (const row of nextOrder) place(end, row.nodes, end, first);
            first = null;
          } else {
            // The rows that stay in the order they had stay put; the others move before the row after them.
            const stayed = nextOrder.filter((row) => !fresh.has(row));
            const before = new Map(order.map((row, index) => [row, index]));
            const places = stayed.map((row) => before.get(row) ?? 0);
            const inOrder = places.every((place, index) => index === 0 || place > (places[index - 1] as number));
            const staying = new Set(inOrder ? stayed : longestRising(places).map((index) => stayed[index]));
            let anchor: Made = end;
            for (let index = nextOrder.length - 1; index >= 0; index--) {
              const row = nextOrder[index] as Row;
              if (!staying.has(row)) place(end, row.nodes, anchor, null);
              anchor = row.nodes[0] ?? anchor;
            }
          }
          rows = next;
          order = nextOrder;
        });
      });
      into.push(end);
    },
  };
}

/** The places in `values` of a longest rising run, in order. */
function longestRising(values: number[]): number[] {
  const tails: number[] = [];
  const previous: number[] = new Array(values.length).fill(-1);
  values.forEach((value, index) => {
    let low = 0;
    let high = tails.length;
    while (low < high) {
      const middle = (low + high) >> 1;
      if ((values[tails[middle] as number] as number) < value) low = middle + 1;
      else high = middle;
    }
    if (low > 0) previous[index] = tails[low - 1] as number;
    tails[low] = index;
  });
  const run: number[] = [];
  for (let at = tails.at(-1) ?? -1; at >= 0; at = previous[at] as number) run.push(at);
  return run.reverse();
}

/** Several children where one goes, as `<>…</>` writes them: a component that gives two shapes. */
export function Fragment(props: { children?: Child }): Region {
  return { mount: (into, family) => materialize(props.children, into, family) };
}

/** Shows `children` while `when` holds, else `fallback`: made again each time it shows. */
export function Show(props: { when: Accessor<unknown>; children: () => Child; fallback?: () => Child }): Region {
  return Switch({ fallback: props.fallback, children: Match(props) });
}

const MATCH = Symbol("match");
type MatchProps = { when: Accessor<unknown>; children: () => Child };

/** A part of a `Switch`, shown while `when` holds and no `Match` before it does. */
export function Match(props: MatchProps): Region {
  return {
    [MATCH]: props,
    mount() {
      throw new Error("a Match goes inside a Switch");
    },
  } as Region;
}

/**
 * Shows the first of its `Match` children whose `when` holds, else `fallback`: made again each
 * time another one shows. `<Switch fallback={() => "None"}><Match when={() => state.mode === "a"}>{() => ...}</Match>...</Switch>`
 */
export function Switch(props: { fallback?: () => Child; children: Child }): Region {
  const matches: MatchProps[] = [];
  const collect = (child: Child) => {
    if (Array.isArray(child)) return child.forEach(collect);
    if (child === null || child === undefined || typeof child === "boolean") return;
    const match = (child as { [MATCH]?: MatchProps })[MATCH];
    if (!match) throw new Error("a Switch holds only Match");
    matches.push(match);
  };
  collect(props.children);
  return {
    mount(into, family) {
      const shown = memo(() => matches.findIndex((match) => Boolean(match.when())));
      dynamic(() => {
        const match = matches[shown()];
        return match ? setup(match.children) : props.fallback ? setup(props.fallback) : null;
      }, into, family);
    },
  };
}

/** What a `Popover` and a `Dialog` take. Its code owns whether it is open: `onClose` sets what `open` reads. */
export interface OverlayProps {
  key?: string;
  open: Accessor<boolean>;
  /** Escape, or a press outside it: of a popover, outside the node it hangs from too. */
  onClose: () => void;
  /** Made again each time it opens, as `Show` makes its part. */
  children: () => Child;
}

const overlay =
  (modal: boolean) =>
  ({ open, children, ...props }: OverlayProps): Region =>
    Show({ when: open, children: () => element("overlay", { ...props, modal }, [children()]) });

/** Content of its own in a panel 8 points in, under the div it is in, as the window's menus hang: a press on the div is its own, so the `onClick` of a button in it opens and closes it. */
export const Popover = overlay(false);
/** Content of its own in a panel 16 points in, over the window, which a dark backdrop covers: a press on the backdrop closes it. While it is open, the view hears no key but those of its text fields. */
export const Dialog = overlay(true);
