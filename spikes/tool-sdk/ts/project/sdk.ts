// The SDK a tool is written against: elements, design tokens, live values, memo and change.
import theme from "./theme.json";

// Design tokens: every name comes from theme.json, so a typo is a type error.
type Theme = typeof theme;
export type ColorToken = keyof Theme["color"];
/** A token, a token at a percent of opacity (`"text/50"`), or `#rrggbb` / `#rrggbbaa`. */
export type Color = ColorToken | `${ColorToken}/${number}` | `#${string}`;
export type TextStyle = keyof Theme["text"];
type Num<K> = K extends `${infer N extends number}` ? N : never;
/** A step of the spacing scale: `gap={2}` is theme.space["2"]. */
export type Space = Num<keyof Theme["space"]>;
export type Radius = keyof Theme["radius"];
export type Shadow = keyof Theme["shadow"];

/** Values the window moves every frame without running the view. `scroll` and `playhead` are in points and ticks, a meter from 0 to 1. */
export type LiveName = "playhead" | "scroll" | "time" | `meter-${number}`;

type Handlers = { onPress?: () => void; onDragEnd?: (moved: Moved) => void };
export type Moved = { dx: number; dy: number };
/** Pressable, draggable and live elements need a `key`: the window and tests find them by it. */
type Keyed =
  | { onPress?: never; drag?: never; onDragEnd?: never; x?: never; level?: never }
  | {
      key: string;
      onPress?: () => void;
      /** The window moves it along `axis` within [min, max] points while dragged; `onDragEnd` hears where it stopped. */
      drag?: { axis: "x" | "y"; min: number; max: number };
      onDragEnd?: (moved: Moved) => void;
      /** Moves it sideways by `offset + scale × live` each frame. */
      x?: { live: LiveName; scale?: number; offset?: number };
      /** Fills it from the bottom by a 0..1 live value; give it a number `w` and `h`. */
      level?: LiveName;
    };

export type BoxProps = Keyed & {
  key?: string;
  /** Points. */
  w?: number | "full";
  h?: number | "full";
  grow?: number;
  pad?: Space;
  px?: Space;
  py?: Space;
  gap?: Space;
  align?: "center";
  justify?: "center" | "between";
  /** Placed at [left, top] points in its parent, out of the flow. */
  at?: [number, number];
  /** Pinned to its parent's edges: [left, top, right, bottom] points, `null` for free. */
  inset?: [number | null, number | null, number | null, number | null];
  clip?: boolean;
  scroll?: boolean;
  /** Lets presses through, to what is under it. */
  inert?: boolean;
  fill?: Color;
  border?: Color;
  /** [top, right, bottom, left] points of `border`; all sides 1 when left out. */
  borderSides?: [number, number, number, number];
  radius?: Radius;
  shadow?: Shadow;
  opacity?: number;
  children?: Child;
};
export type TextProps = { key?: string; at?: [number, number]; style?: TextStyle; color?: Color; w?: number; truncate?: boolean; live?: LiveName; children?: string | number | (string | number)[] };
/** Line segments, `points` four numbers each (x1 y1 x2 y2), all `width` points wide. */
export type Lines = { color: Color; width: number; points: number[] };
/** `units` [across, down]: the points are in units of your own, stretched to `w` × `h`; widths stay in points. */
export type ShapesProps = { key?: string; at?: [number, number]; w: number; h: number; units?: [number, number]; lines: Lines[] };

export type Node =
  | ({ t: "row" | "col" | "box"; press?: true; children: Node[] } & Omit<BoxProps, "children" | "onPress" | "onDragEnd">)
  | ({ t: "text"; text: string } & Omit<TextProps, "children">)
  | ({ t: "shapes" } & ShapesProps)
  | { t: "memo"; key: string };
export type Child = Node | string | number | null | undefined | false | Child[];

declare global {
  namespace JSX {
    type Element = Node;
    interface IntrinsicElements {
      row: BoxProps;
      col: BoxProps;
      box: BoxProps;
      text: TextProps;
      shapes: ShapesProps;
    }
    interface ElementChildrenAttribute {
      children: {};
    }
  }
}

// What one render collects: handlers by key, and the memos it used.
type Scope = { handlers: Map<string, Handlers>; memos: string[] };
type Entry = Scope & { deps: unknown[]; v: number; node: Node };
const cache = new Map<string, Entry>();
let scope: Scope = { handlers: new Map(), memos: [] };
let root = scope;
let fresh: Record<string, { v: number; node: Node }> = {};
let builds = 0;
let last: Node = { t: "box", children: [] };

const flat = (c: Child, out: Node[] = []): Node[] => {
  if (Array.isArray(c)) for (const each of c) flat(each, out);
  else if (typeof c === "string" || typeof c === "number") out.push({ t: "text", text: String(c) });
  else if (c) out.push(c);
  return out;
};

export function h(tag: keyof JSX.IntrinsicElements | ((props: any) => Node), props: Record<string, any> | null, ...children: Child[]): Node {
  if (typeof tag === "function") return tag({ ...props, children: children.length === 1 ? children[0] : children });
  const { onPress, onDragEnd, ...rest } = props ?? {};
  if (onPress || onDragEnd) scope.handlers.set(rest.key, { onPress, onDragEnd });
  if (tag === "text") return { t: "text", ...rest, text: children.flat().join("") } as Node;
  if (tag === "shapes") return { t: "shapes", ...rest } as Node;
  return { t: tag, ...rest, ...(onPress ? { press: true } : {}), children: flat(children) } as Node;
}

/** Builds `build` again only when one of `deps` changed (compared with `Object.is`, as React's); else the window keeps what it has. `key` is unique in the view. */
export function memo(key: string, deps: unknown[], build: () => Node): Node {
  scope.memos.push(key);
  const old = cache.get(key);
  if (old && old.deps.length === deps.length && old.deps.every((d, i) => Object.is(d, deps[i]))) return { t: "memo", key };
  const outer = scope;
  scope = { handlers: new Map(), memos: [] };
  const node = build();
  const entry = { ...scope, deps, v: ++builds, node };
  scope = outer;
  cache.set(key, entry);
  fresh[key] = { v: entry.v, node };
  return { t: "memo", key };
}

let state: unknown;
let changes: Op[] = [];
/** A change to records: the path to a value, and the value. */
export type Op = [path: (string | number)[], value: unknown];

/** Asks the host to change records. Write to `draft` as to the state; the host applies it and the view runs again. */
export function change<S>(edit: (draft: S) => void): void {
  const writes = new Map<string, unknown>();
  const draft = (target: any, path: (string | number)[]): any =>
    new Proxy(target, {
      get(t, p) {
        if (typeof p === "symbol") return t[p];
        const at = [...path, /^\d+$/.test(p) ? Number(p) : p];
        const id = JSON.stringify(at);
        const value = writes.has(id) ? writes.get(id) : t[p];
        return value !== null && typeof value === "object" ? draft(value, at) : value;
      },
      set(_, p, value) {
        const at = [...path, typeof p === "string" && /^\d+$/.test(p) ? Number(p) : (p as string)];
        writes.set(JSON.stringify(at), value);
        changes.push([at, value]);
        return true;
      },
    });
  edit(draft(state, []));
}

// For the host and for tests.

/** Runs the view: the tree, the memos built again, and the memos no longer used. */
export function render<S>(view: (state: S) => Node, s: S) {
  state = s;
  fresh = {};
  scope = root = { handlers: new Map(), memos: [] };
  const node = (last = view(s));
  const used = new Set<string>();
  const mark = (keys: string[]) => keys.forEach((k) => !used.has(k) && (used.add(k), mark(cache.get(k)!.memos)));
  mark(root.memos);
  const drop = [...cache.keys()].filter((k) => !used.has(k));
  drop.forEach((k) => cache.delete(k));
  return { root: node, memos: fresh, drop };
}

/** `state` with `ops` applied, sharing what did not change, so `memo` deps see what changed. */
export function apply<S>(s: S, ops: Op[]): S {
  const set = (at: any, path: (string | number)[], value: unknown): any => {
    if (path.length === 0) return value;
    const [head, ...rest] = path;
    const copy = Array.isArray(at) ? [...at] : { ...at };
    copy[head as any] = set(at?.[head as any], rest, value);
    return copy;
  };
  return ops.reduce((acc, [path, value]) => set(acc, path, value), s);
}

const handlersOf = (key: string) => root.handlers.get(key) ?? [...cache.values()].find((e) => e.handlers.has(key))?.handlers.get(key);

/** Presses `key` as a pointer does; returns the changes it asked for. */
export function press(key: string): Op[] {
  handlersOf(key)?.onPress?.();
  return takeChanges();
}

/** Ends a drag of `key` as the window does; returns the changes it asked for. */
export function dragEnd(key: string, moved: Moved): Op[] {
  handlersOf(key)?.onDragEnd?.(moved);
  return takeChanges();
}

export function takeChanges(): Op[] {
  const out = changes;
  changes = [];
  return out;
}

/** The element `key` of the last render, with memos filled in. */
export function find(key: string, node: Node = last): Node | undefined {
  const at = node.t === "memo" ? cache.get(node.key)?.node : node;
  if (!at) return undefined;
  if ("key" in at && at.key === key) return at;
  if ("children" in at && Array.isArray(at.children))
    for (const c of at.children) {
      const hit = find(key, c);
      if (hit) return hit;
    }
  return undefined;
}
