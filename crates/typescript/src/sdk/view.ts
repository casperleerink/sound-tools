// What a tool's `view` is written with: a plain function of its records, run again after every
// change. Elements, design tokens from `theme.json`, live values, `memo`, changes, and the
// checks a test runs.

import type theme from "../theme.json";
import type { Fields, Inputs, Unit, ValuesOf } from "./fields";
import { registry } from "./tools";
import type { Clock } from "./clock";
import type { ChildTools, Context, FilledStateOfTool, OwnedChild, ProjectTool } from "./tools";
import { Live, splitPath, type Num } from "./values";

// Design tokens: every name comes from theme.json, so a misspelled one is a type error.
type Theme = typeof theme;
export type ColorToken = keyof Theme["color"];
/** A token, a token at a percent of opacity (`"text/50"`), or `#rrggbb` / `#rrggbbaa`. */
export type Color = ColorToken | `${ColorToken}/${number}` | `#${string}`;
export type TextStyle = keyof Theme["text"];
type NumberOf<K> = K extends `${infer N extends number}` ? N : never;
/** A step of the spacing scale: `gap={2}` is theme.space["2"]. */
export type Space = NumberOf<keyof Theme["space"]>;
export type Radius = keyof Theme["radius"];
export type Shadow = keyof Theme["shadow"];

/**
 * Where live values start: numbers the window works out every frame without running the view.
 * Make more with `Live`'s methods, such as `live.peak("t01/mix").hold(3).decibels()`.
 */
export const live = {
  /** Where the piece plays, in ticks: 960 a quarter note. */
  playhead: new Live({ op: "tick" }),
  /** Seconds since the window opened. */
  elapsed: new Live({ op: "elapsed" }),
  /** The loudest sample since the last frame, 0 to 1 and above when it clips, of the instance at `path` inside this one, such as `"t01/mix"`. */
  peak: (path: string): Live => new Live({ op: "peak", name: path }),
  /** A number field of the record as it plays (what a lane plays into it, else the record), or of an instance inside as `"t01/mix.gain_db"`. */
  field: (name: string): Live => new Live({ op: "value", ...splitPath(name), fraction: false }),
};

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

/** Points, or its parent's. A live `w` or `h` places it at `at` (0, 0 when left out), out of the flow, and needs the other in points. */
type Size = { w?: number | "full"; h?: number | "full" } | { w: Num; h: Num };

export type BoxProps = Interactive & Size & {
  key?: string;
  label?: string;
  role?: Role;
  grow?: number;
  pad?: Space;
  px?: Space;
  py?: Space;
  gap?: Space;
  align?: "center" | "end";
  justify?: "center" | "between" | "end";
  /** Placed at [left, top] points in its parent, out of the flow. */
  at?: [Num, Num];
  /** Pinned to its parent's edges: [left, top, right, bottom] points, `null` for free. */
  inset?: [number | null, number | null, number | null, number | null];
  /** The most points tall it grows; past it, what does not fit is cut, or scrolls with `scroll`. */
  maxH?: number;
  clip?: boolean;
  /** Scrolls what does not fit, along `x`, `y` or both; the window fades an edge where more waits. */
  scroll?: "x" | "y" | "both";
  /** Lets presses through, to what is under it. */
  inert?: boolean;
  fill?: Color;
  border?: Color;
  /** [top, right, bottom, left] points of `border`; all sides 1 when left out. */
  borderSides?: [number, number, number, number];
  radius?: Radius;
  shadow?: Shadow;
  opacity?: Num;
  children?: Child;
};
type TextChild = string | number | null | undefined | false | TextChild[];
type TextLook = { key?: string; at?: [Num, Num]; style?: TextStyle; color?: Color; w?: number; truncate?: boolean; opacity?: Num };
/**
 * Its words, or a live value it shows: in `unit`, a tick as `"position"` (`5.3`) or `"time"`
 * (`1:07`), else a plain number with `digits` after the point.
 */
export type TextProps = TextLook &
  (
    | { children?: TextChild; live?: never; unit?: never; digits?: never }
    | { live: Num; unit: Unit | "position" | "time"; digits?: never; children?: never }
    | { live: Num; digits?: number; unit?: never; children?: never }
  );
/** Line segments, `points` four numbers each (x1 y1 x2 y2), all `width` points wide. */
export type Lines = { color: Color; width: number; points: number[] };
/** Filled rectangles, `rects` four numbers each (x y w h). */
export type Rects = { color: Color; rects: number[] };
/** `units` [across, down]: the points are in units of your own, stretched to `w` × `h`; widths stay in points. */
export type ShapesProps = { key?: string; at?: [Num, Num]; w: number; h: number; units?: [number, number]; lines?: Lines[]; rects?: Rects[]; opacity?: Num };
/** The card of a child at `path` inside this tool, such as `t00/synth`: its own view, or one control per field. */
export type ChildProps = { path: string };

export type Node =
  | ({ t: "row" | "col" | "box"; press?: true; actions?: string[]; children: Node[] } & Omit<BoxProps, "children" | "onPress" | "onDragEnd" | "actions">)
  | ({ t: "text"; text: string } & TextLook & { live?: Num; unit?: Unit | "position" | "time"; digits?: number })
  | ({ t: "shapes" } & ShapesProps)
  | ({ t: "child" } & ChildProps)
  | { t: "memo"; key: string };
export type Child = Node | string | number | null | undefined | false | Child[];

/** The tools of the children of `T`, as its `children` names them. */
type KidsOf<T> = T extends ProjectTool<string, infer Entry> ? (Entry extends { children: infer K extends ChildTools } ? K : {}) : ChildTools;
/** An instance of the tool `T` as its view gets it, for a test or a part of a view: its record, every field filled, and its children's. */
export type Self<T> = Drawn<FilledStateOfTool<T>, KidsOf<T>>;
/** An instance as its view gets it: its record, every field filled, and its children's. */
export type Drawn<State, K extends ChildTools = ChildTools> = { name: string; state: State; children: OwnedChild<K>[] };
/** A tool's `view`: what the window shows of an instance, run again whenever its records change. Its handlers use `ctx` as `tick` does: `update`, `send`, `play`, `clock`. */
export type ToolView<S extends Fields, I extends Inputs, M, K extends ChildTools> = (self: Drawn<ValuesOf<S>, K>, ctx: Context<ValuesOf<S>, I, M, K>) => Node;

/** What all code of an instance gets of the view. */
export interface ViewContext<_State, _I extends Inputs, M> {
  /** What its code keeps and nothing saves, made by its tool's `memory`. Changed in a handler, the view runs again. */
  memory: M;
}

type Draw = (self: Drawn<unknown>, ctx: Context<any>) => Node;

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
type Instance = { cache: Map<string, Entry>; root: Scope; last: Node; builds: number };

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
  if (tag === "text") return { t: "text", ...rest, text: words(children as TextChild) } as Node;
  if (tag === "shapes" || tag === "child") return { t: tag, ...rest } as Node;
  const named = actions ? { actions: Object.keys(actions) } : {};
  return { t: tag, ...rest, ...(onPress ? { press: true } : {}), ...named, children: flat(children) } as Node;
}

/** The text of a text's children: without `false`, `null` and `undefined`, as elements leave them out. */
const words = (c: TextChild): string => (Array.isArray(c) ? c.map(words).join("") : typeof c === "string" || typeof c === "number" ? String(c) : "");

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

function instanceOf(id: string): Instance {
  let known = instances.get(id);
  if (!known) {
    known = { cache: new Map(), root: { handlers: new Map(), memos: [] }, last: { t: "box", children: [] }, builds: 0 };
    instances.set(id, known);
  }
  return known;
}

function run(id: string, draw: Draw, self: Drawn<unknown>, ctx: Context<any>): Tree {
  const at = instanceOf(id);
  current = at;
  fresh = {};
  scope = at.root = { handlers: new Map(), memos: [] };
  try {
    at.last = draw(self, ctx);
  } catch (error) {
    // The window keeps no memo of a view that failed: the next run builds every one again.
    at.cache.clear();
    throw error;
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

/** Runs an action of `key`: `press` is its `onPress`, `drag` its `onDragEnd`. Throws when it has none. */
function perform(at: Instance | undefined, key: string, action: string, value: number | Moved | undefined) {
  const handlers = at && handlersOf(at, key);
  if (action === "press" && handlers?.onPress) return handlers.onPress();
  if (action === "drag" && handlers?.onDragEnd) return handlers.onDragEnd(value as Moved);
  const named = handlers?.actions?.[action];
  if (!named) throw new Error(`${key} has no ${action}`);
  if (action === "set" && typeof value !== "number") throw new Error("set takes a number");
  named(value as number);
}

/** What the host runs. Not for tools. */
export const host = {
  run,
  /** Runs `action` of `key` in the view of `id`; throws why it could not. */
  perform: (id: string, key: string, action: string, value?: number | Moved) => perform(instances.get(id), key, action, value),
  forget: (id: string) => instances.delete(id),
};

// For tests: one view at a time, with no window.

const TESTED = "test";
let tested: { tool: ProjectTool; self: Drawn<unknown>; changes: Op[]; memory: unknown } | undefined;

/** Runs the view of `tool` on `self` as the window does, and keeps it for `find`, `press` and `act`. Its memory lasts while the tool is the same. */
export function render<T extends ProjectTool>(tool: T, self: Self<T>): Node {
  const spec = registry.tools.get(String(tool));
  if (!spec?.view) throw new Error(`${String(tool)} has no view`);
  const memory = tested?.tool === tool ? tested.memory : (spec.memory?.() ?? {});
  if (tested?.tool !== tool) instances.delete(TESTED);
  tested = { tool, self: self as Drawn<unknown>, changes: [], memory };
  // A handler a memo kept from an earlier run changes what the last run shows, as in the window.
  // Its draft is a copy of the records with its earlier changes, as the host's is.
  const update = (_label: string, edit: (draft: never) => void) => {
    if (!tested) return;
    const before = apply(tested.self, tested.changes);
    const draft = structuredClone(before);
    edit(draft as never);
    tested.changes.push(...diff(before, draft));
  };
  run(TESTED, spec.view as unknown as Draw, tested.self, quiet(tested.self, memory, update));
  return instances.get(TESTED)!.last;
}

/** What code gets in a test: its records and `update`; what it plays and asks goes nowhere. */
function quiet(self: Drawn<unknown>, memory: unknown, update: Context<unknown>["update"]): Context<any> {
  const nothing = () => {};
  const clock = { playing: false, repeat: null, click: false, computerKeys: false, writeAutomation: false, tempo_map: { time_signatures: [{ signature: "4/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 120 }] }, sections: [], tick: 0, bar: 1, beat: 1, bpm: 120, play: nothing, pause: nothing, stop: nothing, jump: nothing, setRepeat: nothing, setClick: nothing, setComputerKeys: nothing, setWriteAutomation: nothing, update: nothing } as unknown as Clock;
  const asked = async () => null;
  return { state: self.state, children: self.children, memory, update, clock, time: 0, outputs: {}, send: nothing, play: nothing, select: nothing, addOffer: nothing, openWindow: nothing, openFile: asked, render: asked, openMenu: asked, exportNotes: async () => false, confirm: async () => false, offers: async () => ({ offers: [], notes: [], here: null }), numbers: async () => [] } as unknown as Context<any>;
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
  if (key === undefined) throw new Error(`no ${keyOrLabel}`);
  perform(at, key, action, value);
  const { tool, self, changes } = tested;
  render(tool, apply(self, changes) as Self<ProjectTool>);
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

/** What changed from `before` to `after`, as paths to each value that changed. */
function diff(before: unknown, after: unknown, path: (string | number)[] = [], ops: Op[] = []): Op[] {
  const list = Array.isArray(before);
  const object = (value: unknown) => value !== null && typeof value === "object";
  const alike = object(before) && object(after) && list === Array.isArray(after) && (!list || (before as unknown[]).length === (after as unknown[]).length);
  if (!alike) {
    if (!Object.is(before, after)) ops.push([path, after]);
    return ops;
  }
  const [from, to] = [before as Record<string, unknown>, after as Record<string, unknown>];
  for (const k of new Set([...Object.keys(from), ...Object.keys(to)])) diff(from[k], to[k], [...path, list ? Number(k) : k], ops);
  return ops;
}
