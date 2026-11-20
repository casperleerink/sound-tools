// What a tool's `view` is written with: a plain function of its records, run again after every
// change. Elements, design tokens from `theme.json`, live values, `memo`, changes, and the
// checks a test runs.

import type theme from "../theme.json";
import type { Clock, Offer as Offered } from "./clock";
import type { ToolFacts, ToolFeeds } from "./feeds";
import type { Fields, Inputs, Note, Unit, ValuesOf } from "./fields";
import type { IconName } from "./icons";
import type { Tools } from "./records";
import { registry, type ChildTools, type Context, type FilledStateOfTool, type OwnedChild, type ProjectTool, type ToolSpec } from "./tools";
import { Live, splitPath, type LiveNode, type Num } from "./values";

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

/** The names of the feeds of any tool whose value is `V`: a number or a list. */
type FeedNames<V> = { [T in keyof ToolFeeds]: { [N in keyof ToolFeeds[T]]: ToolFeeds[T][N] extends V ? N : never }[keyof ToolFeeds[T]] }[keyof ToolFeeds] & string;
/** A list feed, which only the `points` or `rects` of a `shapes` draw. */
export type FeedList = { readonly node: LiveNode; readonly list: true };

/**
 * Where live values start: numbers the window works out every frame without running the view.
 * Make more with `Live`'s methods, such as `live.feed("peak", "t01/mix").hold(3).decibels()`.
 */
export const live = {
  /** Where the piece plays, in ticks: 960 a quarter note. */
  playhead: new Live({ op: "tick" }),
  /** 1 while the piece plays, else 0: green is sound that moves. */
  playing: new Live({ op: "playing" }),
  /** Seconds since the window opened. */
  elapsed: new Live({ op: "elapsed" }),
  /** The tempo where the piece plays, in beats a minute. */
  bpm: new Live({ op: "bpm" }),
  /** A number field of the record as it plays (what a lane or a drag's `plays` puts in it, else the record), or of an instance inside as `"t01/mix.gain_db"`. */
  field: (name: string): Live => new Live({ op: "value", ...splitPath(name), fraction: false }),
  /**
   * What the instance at `path` inside this one publishes as `name`, or this one when left out:
   * a number, `live.feed("peak", "t01/mix")`, one of a list, `live.feed("sounding.60")`, or a
   * list for a `shapes`, `live.feed("response")`. `sdk/feeds.ts` names each tool's.
   */
  feed: ((name: string, path = "") => new Live({ op: "feed", name, ...(path === "" ? {} : { path }) })) as {
    (name: FeedNames<number> | `${FeedNames<number[]>}.${number}`, path?: string): Live;
    (name: FeedNames<number[]>, path?: string): FeedList;
  },
  /** What the plane `key` of this view shows now, in its units, and how far it is zoomed: 1 as it opened. */
  plane: (key: string) => ({ left: planePart(key, "left"), right: planePart(key, "right"), top: planePart(key, "top"), bottom: planePart(key, "bottom"), zoom: planePart(key, "zoom") }),
  /** The drag `id` (an element's key, or its `drag.id`) now, in its units: how far it moved, where it began (`x`, `y`), 1 while it is and while Alt is held; 0 once its record took it. */
  drag: (id: string) => ({ dx: dragPart(id, "dx"), dy: dragPart(id, "dy"), x: dragPart(id, "startX"), y: dragPart(id, "startY"), active: dragPart(id, "active"), alt: dragPart(id, "alt") }),
};
const dragPart = (id: string, part: "dx" | "dy" | "startX" | "startY" | "active" | "alt") => new Live({ op: "drag", id, part });
const planePart = (key: string, part: "left" | "right" | "top" | "bottom" | "zoom") => new Live({ op: "plane", key, part });

/**
 * A press or the end of a drag: where it began and how far it went (0 for a press), in the units
 * of the plane around it, else points in the element.
 */
export type Pointer = {
  x: number;
  y: number;
  dx: number;
  dy: number;
  /** 2 on the second press of a double click. */
  clicks: number;
  /** The right button, the menu key or Shift-F10. */
  right: boolean;
  shift: boolean;
  alt: boolean;
  cmd: boolean;
  /** At a drag's end: the key of the keyed element of this view under the pointer. */
  over?: string;
};
/** Files dropped from the system: audio already copied into `assets/audio/` (its names there), MIDI read as parts. */
export type Dropped = { x: number; y: number; audio: string[]; parts: { name: string; notes: Note[] }[] };

/** One axis of a plane: points per unit (it scrolls over what is placed in it); a range it shows, stretched to the box (where it opens, if it zooms); or the key of a plane it moves with. */
type Axis = number | [from: number, to: number] | string;
/** A box with units of its own: `place` puts things in it. The window scrolls it (wheel, Shift-wheel) and zooms it across (pinch, Cmd-wheel, about the pointer) within `zoom`. */
export type Plane = {
  x?: Axis;
  y?: Axis;
  zoom?: [min: number, max: number];
  /** A place across it keeps in view while it moves, such as `live.playhead`: the plane pages on when it leaves. A scroll or zoom by hand stops that until it shows again, or rests and moves again. */
  follow?: Num;
};
/** In units of the plane around it, at any depth: `[x, y]` a mark, its `w` and `h` points centred on it; `[x, y, w, h]` a box. */
type Place = [x: Num, y: Num] | [x: Num, y: Num, w: Num, h: Num];

/** Where a drag may go along an axis: anywhere, or the least and most of the move, in units. */
type Along = true | [min: number, max: number];
/** A drag along one axis or both. */
export type Drag = ({ x: Along; y?: Along } | { x?: Along; y: Along }) & {
  /** Grid steps in units: `from` lands on the grid. Cmd held skips it. */
  snap?: [x: number, y: number];
  /** The point that snaps. Left out: the `place` of the element, or of the placed one around it. */
  from?: [x: number, y: number];
  /** What `live.drag(id)` follows. Give the chosen things one id and they move as one. Left out: its key. */
  id?: string;
  /** Numbers of the record (by path, as `live.field` takes them) that play these values while it is held, unsaved: the sound follows the hand. */
  plays?: Record<string, Live>;
};

/** What an element is, for an agent and a screen reader. */
export type Role =
  | "button" | "switch" | "slider" | "tab" | "meter" | "group" | "heading" | "textbox" | "radiogroup" | "radio"
  | "menu" | "menuitem" | "listbox" | "option" | "grid" | "gridcell" | "toolbar" | "dialog" | "tooltip";
/**
 * Named actions an agent runs, and some keys: `toggle`, `set` (a number), `increment` and
 * `decrement` (the arrow keys, by a number of steps: 1, 10 with Page Up and Down, to its end with Home and End), `reset`
 * (a double click, Backspace).
 */
export type Actions = { [name: string]: (value: number) => void };
/** What an action takes: a number, a typed text (`null` when it was left), or a press, a drag or a drop. */
type Value = number | string | null | Partial<Pointer> | Partial<Dropped>;

/** An element a pointer or an agent uses: its `key` is unique in the view, its `label` says what it is. */
type Interactive =
  | { onPress?: never; actions?: never; drag?: never; onDragEnd?: never; onEnter?: never; onDrop?: never }
  | {
      key: string;
      /** What it is called, such as "Mute Track 2": an agent acts on it by this name. */
      label: string;
      role?: Role;
      /** A switch's state. */
      checked?: boolean;
      /** What it shows, such as -6 for a fader in dB, or what plays while a lane moves it. */
      value?: number | string | Live;
      onPress?: (p: Pointer) => void;
      actions?: Actions;
      /** `live.drag(key)` follows it every frame and `onDragEnd` hears where it stopped. A press that went past a few points is a drag and never also presses. */
      drag?: Drag;
      onDragEnd?: (p: Pointer) => void;
      /** A paint: while a press that began on an element with `onEnter` is held, once for it and once for each other one the pointer enters, such as the cells of a grid. All it changes is one step of undo. */
      onEnter?: () => void;
      /** Files dropped on it from the system. Its `hover` look shows while they are over it. */
      onDrop?: (d: Dropped) => void;
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
  /** Placed in the plane around it, out of the flow. Outside a plane it is a problem. */
  place?: Place;
  /** Gives it units of its own, which `place` inside it uses. */
  plane?: Plane;
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
  /** One stop of Tab: the arrow keys go to the nearest control inside in their direction, Home and End to the ends of its row, Escape out. */
  group?: true;
  /**
   * Shown over everything beside its parent, which needs a `key` and is what opens it: a menu, a
   * list to pick from, a tip. `"press"`: a press on its parent or Enter opens and closes it, and a
   * press outside or Escape closes it, as does a press inside when its `role` is `menu` or
   * `listbox`. `"hover"`: shown while the pointer is on its parent or the keys are, letting presses
   * through. `"menu"`: a right press, the menu key or Shift-F10 on its parent, or on anything
   * inside, opens it at the pointer. `gap` is in points; a negative one lays it over its parent.
   */
  overlay?: { open: "press" | "hover" | "menu"; side?: "top" | "bottom" | "left" | "right"; align?: "start" | "center" | "end"; gap?: number };
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
/** Line segments, `points` four numbers each (x1 y1 x2 y2), or a list feed: its numbers are heights, evenly from the left edge to the right, drawn as they change. Segments end to end draw as one line; `fill` closes it and fills it, or under an open line the area below. */
export type Lines = { color: Color; width: number; points: number[] | FeedList; fill?: Color };
/** Filled rectangles, `rects` four numbers each (x y w h), or a list feed. */
export type Rects = { color: Color; rects: number[] | FeedList };
type Span = number | [from: number, to: number];
/**
 * Its points are in `units` [across, down] when given, stretched to `w` × `h`: each a length from
 * 0, or its ends, as `[18, -18]` puts 18 at the top; across, the part of a list feed it shows.
 * Else in the units of the plane when placed; else points. Placed, a list feed reads only what
 * the plane shows. Widths stay in points. `trim` [from, to], 0 to 1: only that part of its
 * lines' length draws, end to end, such as the arc of a knob.
 */
export type ShapesProps = { key?: string; at?: [Num, Num]; units?: [Span, Span]; lines?: Lines[]; rects?: Rects[]; opacity?: Num; trim?: [Num, Num] } & (
  | { w: Num; h: Num; place?: [x: Num, y: Num] }
  | { place: [x: Num, y: Num, w: Num, h: Num]; w?: never; h?: never }
);
/** One line of text the composer types, such as a name or a value: Enter or a press elsewhere gives `onCommit` what it says, Escape gives it `null`. Inside a `menu` or `listbox` overlay, what it says picks the items shown by their labels. */
export type FieldProps = {
  key: string;
  label: string;
  /** What it says while nobody types. */
  value: string;
  onCommit: (text: string | null) => void;
  /** Takes the keys when it appears after a press, its text chosen: a rename in place. */
  focus?: true;
  style?: TextStyle;
  color?: Color;
  w?: number;
  align?: "center" | "end";
  placeholder?: string;
};
/** `extensions/ui/icons/<name>.svg` of the project, `size` points square (16 when left out), tinted `color`. */
export type IconProps = { name: IconName; size?: number; color?: Color };
/** The card of a child at `path` inside this tool, such as `t00/synth`: its own view, or one control per field. */
export type ChildProps = { path: string };

export type Node =
  | ({ t: "row" | "col" | "box"; press?: true; enter?: true; drop?: true; actions?: string[]; children: Node[] } & Omit<BoxProps, "children" | "onPress" | "onDragEnd" | "onEnter" | "onDrop" | "actions">)
  | ({ t: "text"; text: string } & TextLook & { live?: Num; unit?: Unit | "position" | "time"; digits?: number })
  | ({ t: "shapes" } & ShapesProps)
  | ({ t: "field" } & Omit<FieldProps, "onCommit">)
  | ({ t: "icon" } & IconProps)
  | ({ t: "child" } & ChildProps)
  | { t: "memo"; key: string };
export type Child = Node | string | number | null | undefined | false | Child[];

/**
 * A field of a record as a card shows it, by its `name` in the record or the object around it:
 * a number with its range, a switch, a choice of options, an object of fields (`group`), a list
 * of `item`s, or an object of `item`s by key (`map`). `scale` spreads a number over a knob's
 * travel: `log` in octaves, as a frequency; `fader` as a volume.
 */
export type FieldInfo = { name: string; label: string } & (
  | { kind: "number"; min: number; max: number; scale: "linear" | "log" | "fader"; integer: boolean; unit: Unit | null; default: number }
  | { kind: "switch"; default: boolean }
  | { kind: "choice"; options: { value: string | number; text: string }[]; default: string | number }
  | { kind: "group"; fields: FieldInfo[] }
  | { kind: "list"; item: FieldInfo; maxItems: number | null }
  | { kind: "map"; item: FieldInfo }
);
/** What the window knows of an instance of the tool `T` that no record holds and that rarely changes: its title, the fields of its record, and more of some tools. A change runs the view. */
export type Facts<T = unknown> = { title: string; fields: FieldInfo[] } & (T extends keyof ToolFacts ? ToolFacts[T] : {});
type WithFacts<C> = C extends { tool: infer T } ? C & { facts: Facts<T> } : never;
/** A child as code gets it: with its facts, and its own children with theirs. */
export type Seen<C> = C extends { tool: infer T; children: readonly (infer G)[] } ? Omit<C, "children"> & { children: WithFacts<G>[]; facts: Facts<T> } : never;

/** The tools of the children of `T`, as its `children` names them. */
type KidsOf<T> = T extends ProjectTool<string, infer Entry> ? (Entry extends { children: infer K extends ChildTools } ? K : {}) : ChildTools;
/** An instance of the tool `T` as its view gets it, for a test or a part of a view: its record, every field filled, and its children's. */
export type Self<T> = Drawn<FilledStateOfTool<T>, KidsOf<T>, T>;
/** An instance of the tool `T` as its view gets it: its record, every field filled, its children's, and its facts. */
export type Drawn<State, K extends ChildTools = ChildTools, T = unknown> = { name: string; state: State; children: Seen<OwnedChild<K>>[]; facts: Facts<T> };
/** A tool's `view`: what the window shows of an instance, run again whenever its records change. Its handlers use `ctx` as `tick` does: `update`, `send`, `play`, `clock`. */
export type ToolView<S extends Fields, I extends Inputs, M, K extends ChildTools> = (self: Drawn<ValuesOf<S>, K>, ctx: Context<ValuesOf<S>, I, M, K>) => Node;

type CardState<T> = T extends keyof Tools ? FilledStateOfTool<T> : Record<string, unknown>;
/**
 * A card of a device without a view: `Card<"eq">` is the project's card of the built-in tool
 * `eq`, exported as `card` from `ui/blocks/cards/eq.tsx`; `Card` is the generic card of the
 * others, from `ui/blocks/generic-card.tsx`. It gets what a tool's view gets.
 */
export type Card<T extends keyof Tools | undefined = undefined> = (
  self: Drawn<CardState<T>, {}, T>,
  ctx: Context<CardState<T>, Inputs, Record<string, unknown>, {}>,
) => Node;

/** A device the window offers to put in a child, with what it takes: notes it plays, or sound it changes. */
export type Offer = Offered & { takes: "notes" | "audio" };

/** What all code of an instance gets of the view. */
export interface ViewContext<_State, _I extends Inputs, M> {
  /** What its code keeps and nothing saves, made by its tool's `memory`. Changed in a handler, the view runs again. */
  memory: M;
  /**
   * Whether a lane moves the number `name` now, of the record or of an instance inside it as
   * `"t01/mix.gain_db"`: its control shows `live.field(name)`, what plays, and is not the
   * composer's to change: an `update` of it is refused. The view runs again when a lane comes or goes.
   */
  held(name: string): boolean;
  /** The devices the window offers to put in a child: built-in ones, plugins and the tools of the project. `addOffer` puts one. */
  catalog: readonly Offer[];
  /** The key the action `name` of its tool has now, such as `"cmd-r"`, or `null` while it has none. */
  shortcut(name: string): string | null;
  /** Shows a range of the plane `key` of its view, in its units: zoomed across so `x` fills it, as far as it zooms; scrolled down to the middle of `y`. Once the plane shows, if it does not yet. */
  show(key: string, range: { x?: [from: number, to: number]; y?: [from: number, to: number] }): void;
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
      field: FieldProps;
      icon: IconProps;
      child: ChildProps;
    }
    interface ElementChildrenAttribute {
      children: {};
    }
  }
}

type Handlers = {
  onPress?: (p: Pointer) => void;
  onDragEnd?: (p: Pointer) => void;
  onEnter?: () => void;
  onDrop?: (d: Dropped) => void;
  onCommit?: (text: string | null) => void;
  actions?: Actions;
};
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
  const { onPress, onDragEnd, onEnter, onDrop, onCommit, actions, ...rest } = props ?? {};
  if (onPress || onDragEnd || onEnter || onDrop || onCommit || actions) scope.handlers.set(rest.key, { onPress, onDragEnd, onEnter, onDrop, onCommit, actions });
  if (tag === "text") return { t: "text", ...rest, text: words(children as TextChild) } as Node;
  if (tag === "shapes" || tag === "child" || tag === "field" || tag === "icon") return { t: tag, ...rest } as Node;
  const named = actions ? { actions: Object.keys(actions) } : {};
  const kids = flat(children);
  const opens = kids.flatMap((kid) => ("overlay" in kid && kid.overlay ? [kid.overlay.open] : []));
  if (opens.length > 0 && (rest.key === undefined || rest.label === undefined)) throw new Error("what an overlay is in opens it: give it a key and a label");
  if (new Set(opens).size < opens.length) throw new Error(`${rest.key} has two overlays that open alike: one each of press, hover and menu`);
  return { t: tag, ...rest, ...(onPress ? { press: true } : {}), ...(onEnter ? { enter: true } : {}), ...(onDrop ? { drop: true } : {}), ...named, children: kids } as Node;
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

type Draw = (self: Drawn<unknown>, ctx: Context<any>) => Node;

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
const pointer = (p: Partial<Pointer> = {}): Pointer => ({ x: 0, y: 0, dx: 0, dy: 0, clicks: 1, right: false, shift: false, alt: false, cmd: false, ...p });

/** Runs an action of `key`: `press`, `drag`, `drop`, `enter` and `commit` its handlers, else one of its `actions`. Throws when it has none. */
function perform(at: Instance | undefined, key: string, action: string, value: Value | undefined) {
  const handlers = at && handlersOf(at, key);
  if (action === "press" && handlers?.onPress) return handlers.onPress(pointer(value as Partial<Pointer>));
  if (action === "drag" && handlers?.onDragEnd) return handlers.onDragEnd(pointer(value as Partial<Pointer>));
  if (action === "drop" && handlers?.onDrop) return handlers.onDrop({ x: 0, y: 0, audio: [], parts: [], ...(value as Partial<Dropped>) });
  if (action === "enter" && handlers?.onEnter) return handlers.onEnter();
  if (action === "commit" && handlers?.onCommit) return handlers.onCommit(value === null ? null : String(value ?? ""));
  const named = handlers?.actions?.[action];
  if (!named) throw new Error(`${key} has no ${action}`);
  if (action === "set" && typeof value !== "number") throw new Error("set takes a number");
  named(typeof value === "number" ? value : 1);
}

/** What an `update` sees of a record: its state and, when it read them, its children. */
type Part = { state?: unknown; children?: readonly { name: string }[] };
/** The number at `path` of `part`, as `held` names it: `cutoff_hz`, `pads.36.volume_db`, `t01/mix.gain_db`. */
function numberAt(part: Part, path: string): unknown {
  const { child, name } = splitPath(path);
  let at: any = part;
  for (const step of child?.split("/") ?? []) at = at?.children?.find((each: { name: string }) => each.name === step);
  at = at?.state;
  for (const step of name.split(/[.[\]]+/).filter(Boolean)) at = at?.[step];
  return at;
}
/** Throws when a change from `before` to `after` moves a number a lane plays: it is not the code's to change. */
function refuseHeld(held: readonly string[], before: Part, after: Part) {
  const moved = held.find((name) => !Object.is(numberAt(before, name), numberAt(after, name)));
  if (moved !== undefined) throw new Error(`a lane plays ${moved}, so it takes no change`);
}

/** What the host runs. Not for tools. */
export const host = {
  run,
  /** Runs `action` of `key` in the view of `id`; throws why it could not. */
  perform: (id: string, key: string, action: string, value?: Value) => perform(instances.get(id), key, action, value),
  forget: (id: string) => instances.delete(id),
  refuseHeld,
};

// For tests: one view at a time, with no window.

const TESTED = "test";
/** What a test answers when a handler asks: `confirm`, and the file `openFile` chose. */
type Answers = { confirm?: boolean; openFile?: string | null };
type Options<F> = { held?: string[]; facts?: Partial<F>; answers?: Answers };
/** `self` as a test gives it: its facts come from `render`, and those of a child and a grandchild are its own to give. */
type Given<S> = S extends { children: readonly (infer C)[] } ? Omit<S, "facts" | "children"> & { children: readonly GivenChild<C>[] } : S;
type GivenChild<C> = C extends { facts: infer F; children: readonly (infer G)[] } ? Omit<C, "facts" | "children"> & { facts?: Partial<F>; children: readonly Told<G>[] } : never;
type Told<C> = C extends { facts: infer F } ? Omit<C, "facts"> & { facts?: Partial<F> } : never;
type Tested = {
  draw: Draw;
  spec?: ToolSpec<Fields, Inputs, unknown>;
  self: Drawn<unknown>;
  memory: unknown;
  held: readonly string[];
  answers: Answers;
  /** The changes of the last `act`, also those after an `await` in it. */
  ops: Op[];
  acting: boolean;
  /** The key of the element whose `menu` overlay is open. */
  menu?: string;
};
let tested: Tested | undefined;

/**
 * Runs the view of a tool, or a `Card`, on `self` as the window does, and keeps it for `find`,
 * `press` and `act`. `held` names the numbers a lane moves; `facts` are its own, a child's are in
 * `self`; `answers` are what `confirm` and `openFile` get. Its memory lasts while it is the same.
 */
export function render<T extends ProjectTool>(tool: T, self: Given<Self<T>>, options?: Options<Facts<T>>): Node;
export function render<C extends (self: any, ctx: any) => Node>(card: C, self: Given<Parameters<C>[0]>, options?: Options<Parameters<C>[0]["facts"]>): Node;
export function render(drawn: ProjectTool | Draw, given: unknown, { held = [], facts, answers = {} }: Options<Facts> = {}): Node {
  const spec = typeof drawn === "function" ? undefined : registry.tools.get(String(drawn));
  const draw = (typeof drawn === "function" ? drawn : spec?.view) as Draw | undefined;
  if (!draw) throw new Error(`${String(drawn)} has no view`);
  const same = tested?.draw === draw;
  if (!same) instances.delete(TESTED);
  const memory = same ? tested!.memory : (spec?.memory?.() ?? {});
  const { name, children } = given as { name: string; children: readonly Kid[] };
  const self = { ...(given as object), children: told(children), facts: { title: spec?.title ?? name, fields: [], ...facts } } as unknown as Drawn<unknown>;
  tested = { draw, spec, self, memory, held, answers, ops: [], acting: false };
  return redraw(tested);
}

type Kid = { name: string; facts?: object; children?: readonly Kid[] };
/** What `told` made of each list and child a test gave, by depth: the same while it is the same, as in the window. */
const toldAt = [new WeakMap<object, unknown>(), new WeakMap<object, unknown>()];
/** Children as a test gives them, with the facts the window gives where the test leaves them out: of children and grandchildren. */
function told(children: readonly Kid[], depth = 2): Kid[] {
  const known = toldAt[depth - 1]!;
  const kept = <T extends object>(given: object, make: () => T): T => {
    if (!known.has(given)) known.set(given, make());
    return known.get(given) as T;
  };
  return kept(children, () =>
    children.map((child) =>
      kept(child, () => ({ ...child, facts: { title: child.name, fields: [], ...child.facts }, ...(depth > 1 && child.children ? { children: told(child.children, depth - 1) } : {}) })),
    ),
  );
}

function redraw(at: Tested): Node {
  run(TESTED, at.draw, viewed(at.self), contextOf(at));
  return instances.get(TESTED)!.last;
}

/** `self` for a run: a handler that reads its records once they changed, such as one a `memo` kept from this run, fails the test. */
function viewed(self: Drawn<unknown>): Drawn<unknown> {
  const read = <K extends "state" | "children">(name: K) => {
    if (tested?.acting && tested.self[name] !== self[name]) throw new Error(`a handler read \`self.${name}\` of an earlier run of the view, which a memo kept or an update changed: read \`ctx.${name}\``);
    return self[name];
  };
  return { ...self, get state() { return read("state"); }, get children() { return read("children"); } };
}

/** What code gets in a test: its records, `update` and `memory`; what it plays goes nowhere, and what it asks gets `answers`. */
function contextOf(at: Tested): Context<any> {
  // A handler a memo kept from an earlier run, or from an earlier `render`, reads and changes what
  // the last run shows, as in the window.
  const update = (_label: string, edit: (draft: never) => void) => {
    const at = tested!;
    const before = at.self;
    const draft = structuredClone(before);
    edit(draft as never);
    refuseHeld(at.held, before, draft);
    const ops = diff(before, draft);
    at.ops.push(...ops);
    at.self = apply(before, ops);
    // After an `await`, as the window does.
    if (!at.acting) redraw(at);
  };
  const nothing = () => {};
  const clock = { playing: false, repeat: null, click: false, computerKeys: false, writeAutomation: false, tempo_map: { time_signatures: [{ signature: "4/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 120 }] }, sections: [], tick: 0, bar: 1, beat: 1, bpm: 120, play: nothing, pause: nothing, stop: nothing, jump: nothing, setRepeat: nothing, setClick: nothing, setComputerKeys: nothing, setWriteAutomation: nothing, update: nothing } as unknown as Clock;
  return {
    get state() { return tested!.self.state; }, get children() { return tested!.self.children; }, memory: at.memory, update, held: (name: string) => at.held.includes(name), clock, time: 0,
    send: nothing, play: nothing, select: nothing, addOffer: nothing, openWindow: nothing, show: nothing, catalog: [],
    shortcut: (name: string) => at.spec?.actions?.[name]?.keys?.[0] ?? null,
    openFile: async () => at.answers.openFile ?? null, confirm: async () => at.answers.confirm ?? false, render: async () => null, exportNotes: async () => false,
  } as unknown as Context<any>;
}

/** Presses `key` as a pointer does, at `p` in its units: a right press opens its `menu`. The changes it asked for. */
export function press(key: string, p?: Partial<Pointer>): Op[] {
  return act(key, "press", p);
}

/**
 * Runs an action of the element with this key or label as the window does: a press, a drag (in
 * its bounds and on its grid), a drop, a commit of a field's text, or one of its `actions`. Or
 * one of the tool's own actions, `act("song.record")`. The changes it asked for; the view runs again.
 */
export function act(keyOrLabel: string, action: "press" | "drag", p?: Partial<Pointer>): Op[];
export function act(keyOrLabel: string, action: "drop", d?: Partial<Dropped>): Op[];
export function act(keyOrLabel: string, action: "commit", text: string | null): Op[];
export function act(keyOrLabel: string, action: string, value?: number): Op[];
export function act(toolAction: `${string}.${string}`): Op[];
export function act(target: string, action?: string, value?: Value): Op[] {
  const at = tested;
  if (!at) throw new Error("render a view first");
  at.ops = [];
  at.acting = true;
  try {
    if (action === undefined) runAction(at, target);
    else actOn(at, target, action, value);
  } finally {
    at.acting = false;
  }
  redraw(at);
  return at.ops;
}

function runAction(at: Tested, id: string) {
  const [tool, name] = [id.slice(0, id.indexOf(".")), id.slice(id.indexOf(".") + 1)];
  if (at.spec?.name !== tool) throw new Error(`render ${tool} first to run ${id}`);
  const action = at.spec.actions?.[name];
  if (!action) throw new Error(`${tool} has no action ${name}`);
  void action.run(contextOf(at) as never);
}

function actOn(at: Tested, keyOrLabel: string, action: string, value: Value | undefined) {
  const path = pathTo(keyOrLabel);
  const found = path?.at(-1);
  const key = found && "key" in found ? found.key : undefined;
  if (!path || key === undefined) throw new Error(`no ${keyOrLabel}`);
  const view = instances.get(TESTED);
  const p = value as Partial<Pointer> | undefined;
  if (p?.over !== undefined && !pathTo(p.over)) throw new Error(`${p.over} is no key of the view`);
  const menu = [...path].reverse().find((node) => menuIn(node) !== undefined);
  if (action === "press" && p?.right && menu && "key" in menu) {
    at.menu = menu.key;
    if (!(view && handlersOf(view, key)?.onPress)) return;
  }
  const picked = path.find((node) => "overlay" in node && node.overlay?.open === "menu" && (node.role === "menu" || node.role === "listbox"));
  perform(view, key, action, action === "drag" ? moved(path, p) : value);
  if (action === "press" && picked) at.menu = undefined;
}

/** The `menu` overlay among the children of `node`. */
const menuIn = (node: Node) => ("children" in node && Array.isArray(node.children) ? node.children.find((kid) => "overlay" in kid && kid.overlay?.open === "menu") : undefined);

/** A drag by `p` of the element at the end of `path` as the window ends it: on its grid unless Cmd is held, then in its bounds. */
function moved(path: Node[], p: Partial<Pointer> = {}): Partial<Pointer> {
  const node = path.at(-1)!;
  const drag = "drag" in node ? node.drag : undefined;
  if (!drag) throw new Error(`${"key" in node ? node.key : "it"} has no drag`);
  const place = [...path].reverse().flatMap((each) => ("place" in each && each.place ? [each.place] : []))[0];
  const from = (axis: 0 | 1) => {
    const at = drag.from?.[axis] ?? place?.[axis];
    if (at === undefined) throw new Error("a drag that snaps needs a `from`, or a `place` around it");
    return rest(at);
  };
  const snapped = (d: number, axis: 0 | 1) => {
    const step = drag.snap?.[axis];
    return !step || p.cmd ? d : Math.round((from(axis) + d) / step) * step - from(axis);
  };
  const bounded = (d: number, along: Along | undefined) => (along === undefined ? 0 : along === true ? d : Math.min(Math.max(d, Math.min(...along)), Math.max(...along)));
  return { ...p, dx: bounded(snapped(p.dx ?? 0, 0), drag.x), dy: bounded(snapped(p.dy ?? 0, 1), drag.y) };
}

/** A live value while nothing moves: a drag at 0. One that reads what only the window knows throws. */
function rest(value: Num): number {
  if (typeof value === "number") return value;
  const n = value.node;
  const [a, b] = "a" in n ? [() => rest(n.a), () => rest(n.b)] : [() => 0, () => 0];
  switch (n.op) {
    case "plus": return a() + b();
    case "minus": return a() - b();
    case "times": return a() * b();
    case "over": return a() / b();
    case "min": return Math.min(a(), b());
    case "max": return Math.max(a(), b());
    case "negate": return -rest(n.x);
    case "drag": if (n.part !== "startX" && n.part !== "startY") return 0;
  }
  throw new Error(`a test cannot tell where a ${n.op} is: give the drag a \`from\``);
}

/** The element with this key or label in the last run, with memos filled in. What a closed `menu` holds is not found. */
export function find(keyOrLabel: string, node?: Node): Node | undefined {
  return pathTo(keyOrLabel, node)?.at(-1);
}

/** The elements from `node` to the one with this key or label, as `find` finds it. */
function pathTo(keyOrLabel: string, node = instances.get(TESTED)?.last, opener?: string): Node[] | undefined {
  const shown = node?.t === "memo" ? instances.get(TESTED)?.cache.get(node.key)?.node : node;
  if (!shown) return undefined;
  if ("overlay" in shown && shown.overlay?.open === "menu" && opener !== tested?.menu) return undefined;
  if (("key" in shown && shown.key === keyOrLabel) || ("label" in shown && shown.label === keyOrLabel)) return [shown];
  if ("children" in shown && Array.isArray(shown.children))
    for (const each of shown.children) {
      const hit = pathTo(keyOrLabel, each, "key" in shown ? shown.key : undefined);
      if (hit) return [shown, ...hit];
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
