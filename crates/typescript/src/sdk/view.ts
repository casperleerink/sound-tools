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
  /** 1 while the piece plays, else 0: green is sound that moves. */
  playing: new Live({ op: "playing" }),
  /** Seconds since the window opened. */
  elapsed: new Live({ op: "elapsed" }),
  /** The loudest sample since the last frame, 0 to 1 and above when it clips, of the instance at `path` inside this one, such as `"t01/mix"`. */
  peak: (path: string): Live => new Live({ op: "peak", name: path }),
  /** 1 while the key `pitch` (60 is middle C) is down on this tool's sound, or on that of the tool of the project at `path` inside it: what a clip, a key or its code plays. For a key that lights: `opacity={live.sounding(60)}`. */
  sounding: (pitch: number, path?: string): Live => new Live({ op: "sounding", pitch, ...(path === undefined ? {} : { child: path }) }),
  /** A number field of the record as it plays (what a lane plays into it, else the record), or of an instance inside as `"t01/mix.gain_db"`. */
  field: (name: string): Live => new Live({ op: "value", ...splitPath(name), fraction: false }),
  /** How far the element `key` of this view is dragged now, in points within its `drag` bounds, and 1 while it is: 0 once its record took the drag. */
  drag: (key: string) => ({ dx: dragPart(key, "dx"), dy: dragPart(key, "dy"), active: dragPart(key, "active") }),
};
const dragPart = (id: string, part: "dx" | "dy" | "active") => new Live({ op: "drag", id, part });

/** What an element is, for an agent and a screen reader. */
export type Role =
  | "button" | "switch" | "slider" | "tab" | "meter" | "group" | "heading" | "textbox" | "radiogroup" | "radio"
  | "menu" | "menuitem" | "listbox" | "option" | "grid" | "gridcell" | "toolbar" | "dialog" | "tooltip";
/**
 * Named actions an agent runs, and some keys: `toggle`, `set` (a number), `increment` and
 * `decrement` (the arrow keys, by a number of steps: 1, or 10 with Page Up and Down), `reset`
 * (a double click, Backspace).
 */
export type Actions = { [name: string]: (value: number) => void };
export type Moved = { dx: number; dy: number };
/** What an action takes: a number, a typed text, or how far a drag moved. */
type Value = number | string | Moved;

/** An element a pointer or an agent uses: its `key` is unique in the view, its `label` says what it is. */
type Interactive =
  | { onPress?: never; actions?: never; drag?: never; onDragEnd?: never; onEnter?: never }
  | {
      key: string;
      /** What it is called, such as "Mute Track 2": an agent acts on it by this name. */
      label: string;
      role?: Role;
      /** A switch's state. */
      checked?: boolean;
      /** What it shows, such as -6 for a fader in dB, or what plays while a lane moves it. */
      value?: number | string | Live;
      onPress?: () => void;
      actions?: Actions;
      /**
       * A drag along `axis`, within [min, max] points: `live.drag(key)` follows it every frame, such as a thumb's `at`, and `onDragEnd` hears
       * where it stopped. A press that went past a few points is a drag and never also presses.
       */
      drag?: { axis: "x" | "y"; min: number; max: number };
      onDragEnd?: (moved: Moved) => void;
      /** A paint: while a press that began on an element with `onEnter` is held, once for it and once for each other one the pointer enters, such as the cells of a grid. All it changes is one step of undo. */
      onEnter?: () => void;
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
  /** Where its children sit across: stretched to fill it when left out. */
  align?: "start" | "center" | "end";
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
  /** A fill over it while the pointer is on it. */
  hover?: Color;
  /** Its parts with `reveal` show only while the pointer is on it or the keys are inside it: the quiet rule. */
  hoverGroup?: true;
  /** Shown only while the `hoverGroup` around it is pointed at or has the keys. */
  reveal?: true;
  /** One stop of Tab: the arrow keys move between the controls inside, such as segments or pads. */
  group?: true;
  /**
   * Shown over everything beside its parent, which needs a `key` and is what opens it: a menu, a
   * list to pick from, a tip. `"press"`: a press on its parent or Enter opens and closes it, and a
   * press outside or Escape closes it, as does a press inside when its `role` is `menu` or
   * `listbox`. `"hover"`: shown while the pointer is on its parent or the keys are, letting presses
   * through. `gap` is in points; a negative one lays it over its parent.
   */
  overlay?: { open: "press" | "hover"; side?: "top" | "bottom" | "left" | "right"; align?: "start" | "center" | "end"; gap?: number };
  border?: Color;
  /** [top, right, bottom, left] points of `border`; all sides 1 when left out. */
  borderSides?: [number, number, number, number];
  radius?: Radius;
  shadow?: Shadow;
  opacity?: Num;
  children?: Child;
};
type TextChild = string | number | null | undefined | false | TextChild[];
/** `align` places it in its `w`. */
type TextLook = { key?: string; at?: [Num, Num]; style?: TextStyle; color?: Color; w?: number; align?: "center" | "end"; truncate?: boolean; opacity?: Num };
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
/** Line segments, `points` four numbers each (x1 y1 x2 y2), all `width` points wide. Segments end to end draw as one line. */
export type Lines = { color: Color; width: number; points: number[] };
/** Filled rectangles, `rects` four numbers each (x y w h). */
export type Rects = { color: Color; rects: number[] };
/**
 * `units` [across, down]: the points are in units of your own, stretched to `w` × `h`; widths stay in points.
 * `trim` [from, to], 0 to 1: only that part of its lines' length draws, end to end in their order, such as the arc of a knob.
 */
export type ShapesProps = { key?: string; at?: [Num, Num]; w: number; h: number; units?: [number, number]; lines?: Lines[]; rects?: Rects[]; opacity?: Num; trim?: [Num, Num] };
/** One line of text the composer types, such as a name or a value: Enter gives `onCommit` what it says; Escape or a press elsewhere leaves it. */
export type FieldProps = {
  key: string;
  label: string;
  /** What it says while nobody types. */
  value: string;
  onCommit: (text: string) => void;
  style?: TextStyle;
  color?: Color;
  w?: number;
  align?: "center" | "end";
  placeholder?: string;
};
/** The card of a child at `path` inside this tool, such as `t00/synth`: its own view, or one control per field. */
export type ChildProps = { path: string };

export type Node =
  | ({ t: "row" | "col" | "box"; press?: true; enter?: true; actions?: string[]; children: Node[] } & Omit<BoxProps, "children" | "onPress" | "onDragEnd" | "onEnter" | "actions">)
  | ({ t: "text"; text: string } & TextLook & { live?: Num; unit?: Unit | "position" | "time"; digits?: number })
  | ({ t: "shapes" } & ShapesProps)
  | ({ t: "field" } & Omit<FieldProps, "onCommit">)
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

/**
 * The view of the card of every device whose tool has none, such as a built-in one: the project
 * exports it as `card` from `ui/blocks/generic-card.tsx`. `fields` are those of its record, in
 * their order; its `update` changes the record.
 */
export type GenericCard = (
  self: {
    name: string;
    state: Record<string, unknown>;
    card: {
      title: string;
      fields: ({ name: string; label: string } & (
        | { kind: "number"; min: number; max: number; log: boolean; integer: boolean; unit: Unit | null; default: number }
        | { kind: "switch"; default: boolean }
        | { kind: "choice"; options: { value: string | number; text: string }[]; default: string | number }
      ))[];
    };
  },
  ctx: { update: (label: string, change: (draft: { state: Record<string, unknown> }) => void) => void; memory: Record<string, unknown>; held: (name: string) => boolean },
) => Node;

/** What all code of an instance gets of the view. */
export interface ViewContext<_State, _I extends Inputs, M> {
  /** What its code keeps and nothing saves, made by its tool's `memory`. Changed in a handler, the view runs again. */
  memory: M;
  /**
   * Whether a lane moves the number `name` now, of the record or of an instance inside it as
   * `"t01/mix.gain_db"`: its control shows `live.field(name)`, what plays, and is not the
   * composer's to change. The view runs again when a lane comes or goes.
   */
  held(name: string): boolean;
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
      field: FieldProps;
      child: ChildProps;
    }
    interface ElementChildrenAttribute {
      children: {};
    }
  }
}

type Handlers = { onPress?: () => void; onDragEnd?: (moved: Moved) => void; onEnter?: () => void; onCommit?: (text: string) => void; actions?: Actions };
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
  const { onPress, onDragEnd, onEnter, onCommit, actions, ...rest } = props ?? {};
  if (onPress || onDragEnd || onEnter || onCommit || actions) scope.handlers.set(rest.key, { onPress, onDragEnd, onEnter, onCommit, actions });
  if (tag === "text") return { t: "text", ...rest, text: words(children as TextChild) } as Node;
  if (tag === "shapes" || tag === "child" || tag === "field") return { t: tag, ...rest } as Node;
  const named = actions ? { actions: Object.keys(actions) } : {};
  const kids = flat(children);
  const opens = kids.flatMap((kid) => ("overlay" in kid && kid.overlay ? [kid.overlay.open] : []));
  if (opens.length > 0 && (rest.key === undefined || rest.label === undefined)) throw new Error("what an overlay is in opens it: give it a key and a label");
  if (new Set(opens).size < opens.length) throw new Error(`${rest.key} has two overlays that open alike: one opens on a press, one on hover`);
  return { t: tag, ...rest, ...(onPress ? { press: true } : {}), ...(onEnter ? { enter: true } : {}), ...named, children: kids } as Node;
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

/** Runs an action of `key`: `press` is its `onPress`, `drag` its `onDragEnd`, `enter` its `onEnter`, `commit` its `onCommit`. Throws when it has none. */
function perform(at: Instance | undefined, key: string, action: string, value: Value | undefined) {
  const handlers = at && handlersOf(at, key);
  if (action === "press" && handlers?.onPress) return handlers.onPress();
  if (action === "drag" && handlers?.onDragEnd) return handlers.onDragEnd(value as Moved);
  if (action === "enter" && handlers?.onEnter) return handlers.onEnter();
  if (action === "commit" && handlers?.onCommit) return handlers.onCommit(String(value ?? ""));
  const named = handlers?.actions?.[action];
  if (!named) throw new Error(`${key} has no ${action}`);
  if (action === "set" && typeof value !== "number") throw new Error("set takes a number");
  named((typeof value === "number" ? value : 1));
}

/** What the host runs. Not for tools. */
export const host = {
  run,
  /** Runs `action` of `key` in the view of `id`; throws why it could not. */
  perform: (id: string, key: string, action: string, value?: Value) => perform(instances.get(id), key, action, value),
  forget: (id: string) => instances.delete(id),
};

// For tests: one view at a time, with no window.

const TESTED = "test";
let tested: { tool: ProjectTool; self: Drawn<unknown>; changes: Op[]; memory: unknown; held: string[] } | undefined;

/** Runs the view of `tool` on `self` as the window does, and keeps it for `find`, `press` and `act`; `held` names the numbers a lane moves. Its memory lasts while the tool is the same. */
export function render<T extends ProjectTool>(tool: T, self: Self<T>, { held = [] }: { held?: string[] } = {}): Node {
  const spec = registry.tools.get(String(tool));
  if (!spec?.view) throw new Error(`${String(tool)} has no view`);
  const memory = tested?.tool === tool ? tested.memory : (spec.memory?.() ?? {});
  if (tested?.tool !== tool) instances.delete(TESTED);
  tested = { tool, self: self as Drawn<unknown>, changes: [], memory, held };
  // A handler a memo kept from an earlier run changes what the last run shows, as in the window.
  // Its draft is a copy of the records with its earlier changes, as the host's is.
  const update = (_label: string, edit: (draft: never) => void) => {
    if (!tested) return;
    const before = apply(tested.self, tested.changes);
    const draft = structuredClone(before);
    edit(draft as never);
    tested.changes.push(...diff(before, draft));
  };
  run(TESTED, spec.view as unknown as Draw, tested.self, quiet(tested.self, memory, update, held));
  return instances.get(TESTED)!.last;
}

/** What code gets in a test: its records and `update`; what it plays and asks goes nowhere. */
function quiet(self: Drawn<unknown>, memory: unknown, update: Context<unknown>["update"], held: string[]): Context<any> {
  const nothing = () => {};
  const clock = { playing: false, repeat: null, click: false, computerKeys: false, writeAutomation: false, tempo_map: { time_signatures: [{ signature: "4/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 120 }] }, sections: [], tick: 0, bar: 1, beat: 1, bpm: 120, play: nothing, pause: nothing, stop: nothing, jump: nothing, setRepeat: nothing, setClick: nothing, setComputerKeys: nothing, setWriteAutomation: nothing, update: nothing } as unknown as Clock;
  const asked = async () => null;
  return { state: self.state, children: self.children, memory, update, held: (name: string) => held.includes(name), clock, time: 0, outputs: {}, send: nothing, play: nothing, select: nothing, addOffer: nothing, openWindow: nothing, openFile: asked, render: asked, openMenu: asked, exportNotes: async () => false, confirm: async () => false, offers: async () => ({ offers: [], notes: [], here: null }), numbers: async () => [] } as unknown as Context<any>;
}

/** Presses `key` as a pointer does; the changes it asked for. The view runs again, as in the window. */
export function press(key: string): Op[] {
  return act(key, "press");
}

/** Runs the action of the element with this key or label, as an agent does; the changes it asked for. */
export function act(keyOrLabel: string, action: string, value?: Value): Op[] {
  if (!tested) throw new Error("render a view first");
  const at = instances.get(TESTED);
  const found = find(keyOrLabel);
  const key = found && "key" in found ? found.key : undefined;
  if (key === undefined) throw new Error(`no ${keyOrLabel}`);
  perform(at, key, action, value);
  const { tool, self, changes, held } = tested;
  render(tool, apply(self, changes) as Self<ProjectTool>, { held });
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
