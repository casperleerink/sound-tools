// What a tool's view is written with in the new window: a plain function of its records, run
// again after every change. Elements, design tokens from `theme.json`, live values, `memo`,
// changes, and the checks a test runs.

import type theme from "../theme.json";
import type { ChildTools, FilledStateOfTool, OwnedChild, Part, ProjectTool } from "../sdk";

// Design tokens: every name comes from theme.json, so a misspelled one is a type error.
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

/**
 * Values the window moves every frame without running the view: `playhead` in ticks, `time` as
 * text, `meter:<path>` the level of the child at that path (`meter:t01/mix`) from 0 to 1.
 */
export type LiveName = "playhead" | "time" | `meter:${string}`;

/** What an element is, for an agent and a screen reader. */
export type Role = "button" | "switch" | "slider" | "tab" | "meter" | "group" | "heading";
/** Named actions an agent runs, and a press: `toggle`, `set` (a number), `increment`, `decrement`. */
export type Actions = { [name: string]: (value: number) => void };
export type Moved = { dx: number; dy: number };

/** An element a pointer or an agent uses: its `key` is unique in the view, its `label` says what it is. */
type Interactive =
  | { onPress?: never; actions?: never; drag?: never; onDragEnd?: never }
  | {
      key: string;
      /** What it is called, such as "Mute Track 2": an agent acts on it by this name. */
      label: string;
      role?: Role;
      /** A switch's state. */
      checked?: boolean;
      /** What it shows, such as -6 for a fader in dB. */
      value?: number | string;
      onPress?: () => void;
      actions?: Actions;
      /** The window moves it along `axis` within [min, max] points while dragged; `onDragEnd` hears where it stopped. */
      drag?: { axis: "x" | "y"; min: number; max: number };
      onDragEnd?: (moved: Moved) => void;
    };

export type BoxProps = Interactive & {
  key?: string;
  label?: string;
  role?: Role;
  /** Points. */
  w?: number | "full";
  h?: number | "full";
  grow?: number;
  pad?: Space;
  px?: Space;
  py?: Space;
  gap?: Space;
  align?: "center" | "end";
  justify?: "center" | "between" | "end";
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
  /** Moves it sideways by `offset + scale × live` points each frame. */
  x?: { live: LiveName; scale?: number; offset?: number };
  /** Fills it from the bottom by a 0..1 live value; give it a number `w` and `h`. */
  level?: LiveName;
  children?: Child;
};
export type TextProps = { key?: string; at?: [number, number]; style?: TextStyle; color?: Color; w?: number; truncate?: boolean; live?: LiveName; children?: string | number | (string | number)[] };
/** Line segments, `points` four numbers each (x1 y1 x2 y2), all `width` points wide. */
export type Lines = { color: Color; width: number; points: number[] };
/** Filled rectangles, `rects` four numbers each (x y w h). */
export type Rects = { color: Color; rects: number[] };
/** `units` [across, down]: the points are in units of your own, stretched to `w` × `h`; widths stay in points. */
export type ShapesProps = { key?: string; at?: [number, number]; w: number; h: number; units?: [number, number]; lines?: Lines[]; rects?: Rects[] };
/** The card of a child at `path` inside this tool, such as `t00/synth`: its own view, or one control per field. */
export type ChildProps = { path: string };

export type Node =
  | ({ t: "row" | "col" | "box"; press?: true; actions?: string[]; children: Node[] } & Omit<BoxProps, "children" | "onPress" | "onDragEnd" | "actions">)
  | ({ t: "text"; text: string } & Omit<TextProps, "children">)
  | ({ t: "shapes" } & ShapesProps)
  | ({ t: "child" } & ChildProps)
  | { t: "memo"; key: string };
export type Child = Node | string | number | null | undefined | false | Child[];

/** The tools of the children of `T`, as its `children` names them. */
type KidsOf<T> = T extends ProjectTool<string, infer Entry> ? (Entry extends { children: infer K extends ChildTools } ? K : {}) : ChildTools;
/** An instance as its view gets it: its record, with every field filled, and its children's. */
export type Self<T> = { name: string; state: FilledStateOfTool<T>; children: OwnedChild<KidsOf<T>>[] };
/** What a view gets besides its records: what it keeps while the window is open, and `change`. */
export type Ctx<T, M> = {
  /** Kept while the window is open, never saved: what is chosen, what is open. Change it in a handler. */
  memory: M;
  /** Changes records: write to `draft` as to the records. One step of undo, named `label`. */
  change: (edit: (draft: Part<FilledStateOfTool<T>, KidsOf<T>>) => void, label?: string) => void;
};
export type View<T = any, M = any> = { tool: string; draw: (self: Self<T>, ctx: Ctx<T, M>) => Node; memory: () => M };

/** The view of `tool` in the new window. `memory` makes what it keeps while the window is open. */
export function view<T extends ProjectTool, M = {}>(tool: T, draw: (self: Self<T>, ctx: Ctx<T, M>) => Node, { memory }: { memory?: () => M } = {}): View<T, M> {
  const made: View<T, M> = { tool: String(tool), draw, memory: memory ?? (() => ({}) as M) };
  views.set(made.tool, made);
  return made;
}

// JSX through `h`: a file imports `h` from here.

export declare namespace h {
  namespace JSX {
    type Element = Node;
    interface IntrinsicElements {
      row: BoxProps;
      col: BoxProps;
      box: BoxProps;
      text: TextProps;
      shapes: ShapesProps;
      child: ChildProps;
    }
    interface ElementChildrenAttribute {
      children: {};
    }
  }
}

type Handlers = { onPress?: () => void; onDragEnd?: (moved: Moved) => void; actions?: Actions };
type Scope = { handlers: Map<string, Handlers>; memos: string[] };
type Entry = Scope & { deps: unknown[]; v: number; node: Node };
/** What a view of one instance keeps between its runs. */
type Instance = { cache: Map<string, Entry>; root: Scope; last: Node; memory: unknown; builds: number };

const views = new Map<string, View>();
const instances = new Map<string, Instance>();
let scope: Scope = { handlers: new Map(), memos: [] };
let current: Instance | undefined;
let fresh: Record<string, { v: number; node: Node }> = {};

const flat = (c: Child, out: Node[] = []): Node[] => {
  if (Array.isArray(c)) for (const each of c) flat(each, out);
  else if (typeof c === "string" || typeof c === "number") out.push({ t: "text", text: String(c) });
  else if (c) out.push(c);
  return out;
};

export function h(tag: keyof h.JSX.IntrinsicElements | ((props: any) => Node), props: Record<string, any> | null, ...children: Child[]): Node {
  if (typeof tag === "function") return tag({ ...props, children: children.length === 1 ? children[0] : children });
  const { onPress, onDragEnd, actions, ...rest } = props ?? {};
  if (onPress || onDragEnd || actions) scope.handlers.set(rest.key, { onPress, onDragEnd, actions });
  if (tag === "text") return { t: "text", ...rest, text: children.flat().join("") } as Node;
  if (tag === "shapes" || tag === "child") return { t: tag, ...rest } as Node;
  const named = actions ? { actions: Object.keys(actions) } : {};
  return { t: tag, ...rest, ...(onPress ? { press: true } : {}), ...named, children: flat(children) } as Node;
}

export function Fragment({ children }: { children?: Child }): Node {
  return { t: "box", children: flat(children) };
}

/** Builds `build` again only when one of `deps` changed (by `Object.is`); else the window keeps what it has. `key` is unique in the view. */
export function memo(key: string, deps: unknown[], build: () => Node): Node {
  const at = current;
  if (!at) return build();
  scope.memos.push(key);
  const old = at.cache.get(key);
  if (old && old.deps.length === deps.length && old.deps.every((d, i) => Object.is(d, deps[i]))) return { t: "memo", key };
  const outer = scope;
  scope = { handlers: new Map(), memos: [] };
  const node = build();
  const entry = { ...scope, deps, v: ++at.builds, node };
  scope = outer;
  at.cache.set(key, entry);
  fresh[key] = { v: entry.v, node };
  return { t: "memo", key };
}

/** A change to records in a test: the path to a value, and the value. */
export type Op = [path: (string | number)[], value: unknown];
/** The tree of one run of a view, the memos it built again, and those it no longer uses. */
export type Tree = { root: Node; memos: Record<string, { v: number; node: Node }>; drop: string[] };

function instanceOf(id: string, made: View): Instance {
  let known = instances.get(id);
  if (!known) {
    known = { cache: new Map(), root: { handlers: new Map(), memos: [] }, last: { t: "box", children: [] }, memory: made.memory(), builds: 0 };
    instances.set(id, known);
  }
  return known;
}

function run(id: string, made: View, self: Self<any>, change: Ctx<any, any>["change"]): Tree {
  const at = instanceOf(id, made);
  current = at;
  fresh = {};
  scope = at.root = { handlers: new Map(), memos: [] };
  try {
    at.last = made.draw(self, { memory: at.memory, change });
  } finally {
    current = undefined;
  }
  const used = new Set<string>();
  const mark = (keys: string[]) => keys.forEach((k) => !used.has(k) && (used.add(k), mark(at.cache.get(k)?.memos ?? [])));
  mark(at.root.memos);
  const drop = [...at.cache.keys()].filter((k) => !used.has(k));
  drop.forEach((k) => at.cache.delete(k));
  return { root: at.last, memos: fresh, drop };
}

const handlersOf = (at: Instance, key: string) => at.root.handlers.get(key) ?? [...at.cache.values()].find((e) => e.handlers.has(key))?.handlers.get(key);

/** Runs an action of `key`: `press` is its `onPress`, `drag` its `onDragEnd`; false when it has none. */
function perform(at: Instance | undefined, key: string, action: string, value: number | Moved | undefined): boolean {
  const handlers = at && handlersOf(at, key);
  if (action === "press" && handlers?.onPress) return handlers.onPress(), true;
  if (action === "drag" && handlers?.onDragEnd) return handlers.onDragEnd(value as Moved), true;
  const named = handlers?.actions?.[action];
  if (!named) return false;
  named(typeof value === "number" ? value : 0);
  return true;
}

/** What the host runs. Not for tools. */
export const host = {
  views,
  run,
  /** Runs `action` of `key` in the view of `id`. */
  perform: (id: string, key: string, action: string, value?: number | Moved) => perform(instances.get(id), key, action, value),
  forget: (id: string) => instances.delete(id),
};

// For tests: one view at a time, with no window.

const TESTED = "test";
let tested: { made: View; self: Self<any>; changes: Op[] } | undefined;

/** Runs `made` on `self` as the window does, and keeps it for `find`, `press` and `act`. Memory lasts across runs. */
export function render<T, M>(made: View<T, M>, self: Self<T>): Node {
  if (tested?.made !== made) instances.delete(TESTED);
  tested = { made, self, changes: [] };
  const recorded = tested;
  run(TESTED, made, self, (edit) => edit(recording(self, recorded.changes)));
  return instances.get(TESTED)!.last;
}

/** Presses `key` as a pointer does; the changes it asked for. The view runs again, as in the window. */
export function press(key: string): Op[] {
  return act(key, "press");
}

/** Runs the action of the element with this key or label, as an agent does; the changes it asked for. */
export function act(keyOrLabel: string, action: string, value?: number | Moved): Op[] {
  if (!tested) throw new Error("render a view first");
  const at = instances.get(TESTED);
  const found = find(keyOrLabel);
  const key = found && "key" in found ? found.key : undefined;
  if (key === undefined || !perform(at, key, action, value)) throw new Error(`no ${keyOrLabel} with the action ${action}`);
  const { made, self, changes } = tested;
  render(made, apply(self, changes));
  return changes;
}

/** The element with this key or label in the last run, with memos filled in. */
export function find(keyOrLabel: string, node?: Node): Node | undefined {
  const at = node ?? instances.get(TESTED)?.last;
  const shown = at?.t === "memo" ? instances.get(TESTED)?.cache.get(at.key)?.node : at;
  if (!shown) return undefined;
  if (("key" in shown && shown.key === keyOrLabel) || ("label" in shown && shown.label === keyOrLabel)) return shown;
  if ("children" in shown && Array.isArray(shown.children))
    for (const each of shown.children) {
      const hit = find(keyOrLabel, each);
      if (hit) return hit;
    }
  return undefined;
}

/** `self` with `ops` applied, sharing what did not change. */
export function apply<S>(self: S, ops: Op[]): S {
  const set = (at: any, path: (string | number)[], value: unknown): any => {
    if (path.length === 0) return value;
    const [head, ...rest] = path;
    const copy = Array.isArray(at) ? [...at] : { ...at };
    copy[head as any] = set(at?.[head as any], rest, value);
    return copy;
  };
  return ops.reduce((acc, [path, value]) => set(acc, path, value), self);
}

/** A draft of `target` whose writes land in `ops`, as paths. */
function recording(target: any, ops: Op[], path: (string | number)[] = []): any {
  const written = new Map<string, unknown>();
  return new Proxy(target, {
    get(t, p) {
      if (typeof p === "symbol") return t[p];
      const at = [...path, /^\d+$/.test(p) ? Number(p) : p];
      const id = JSON.stringify(at);
      const value = written.has(id) ? written.get(id) : t[p];
      return value !== null && typeof value === "object" ? recording(value, ops, at) : value;
    },
    set(_, p, value) {
      const at = [...path, typeof p === "string" && /^\d+$/.test(p) ? Number(p) : (p as string)];
      written.set(JSON.stringify(at), value);
      ops.push([at, value]);
      return true;
    },
  });
}
