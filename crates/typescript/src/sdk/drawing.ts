// What a card draws.

import { labelOf, SelectOption, valueOf } from "./components";
import { Unit } from "./fields";
import { Drag, Dropped, KeyName, Live, Num, PointerEvents, Shortcut, TakeList, WaveList } from "./live";
import { Child, element, Region, UiNode } from "./nodes";
import { Bind, setup, untrack } from "./reactivity";
import type { Input as InputOfTool } from "./fields";
import type { Notes as NotesOfSound } from "./sound";
import type { Key } from "./tools";

/** `Input` is also the type of an input of a tool, `Notes` that of notes a sound sends. */
export type Input = InputOfTool;
export type Notes = NotesOfSound;

/**
 * A colour of the app's theme, by what it is for. `background` is the window, `surface` a card
 * or a panel on it, `well` a display inside a card. `fill` is a control at rest or a quiet box,
 * `fill-strong` one under the pointer and the track of a knob, `selected` the row that is chosen.
 * `border` is a hairline between rows, `border-strong` the outline of a field. `text` is text
 * and a value, `text-secondary` a label, `text-muted` a hint or a unit, `text-inverse` text on a
 * bright fill. A hue means one thing: `green` sound that moves, `yellow` solo, `peach` a warning
 * and mute, `red` record and errors, `lavender` the keyboard focus. The other hues are marks,
 * such as the colour of a track.
 */
export type ThemeColor =
  | "background" | "surface" | "well" | "fill" | "fill-strong" | "selected" | "border" | "border-strong"
  | "text" | "text-secondary" | "text-muted" | "text-inverse" | "none"
  | "blue" | "sapphire" | "sky" | "teal" | "green" | "yellow" | "peach" | "red" | "maroon" | "mauve" | "pink" | "lavender" | "rosewater" | "flamingo";
/** A colour of the theme, or one at an opacity in percent, `"green/40"`. `#rrggbb` and `#rrggbbaa` are for what the theme cannot say. */
export type Color = ThemeColor | `${ThemeColor}/${number}` | `#${string}`;

/** `color` at an opacity in percent, as `"green/40"` writes it, in place of the one it had: for a colour that is a variable, such as one kept in a list. */
export const alpha = (color: Color, percent: number): Color => `${color.split("/")[0] as ThemeColor}/${percent}`;

/** Room between things and inside them: 4, 8, 12, 16, 24 or 32 points. */
export type Spacing = "xs" | "sm" | "md" | "lg" | "xl" | "2xl";
/** How round a corner is: 4, 6 or 8 points, or `full`, round at its ends. */
export type Radius = "sm" | "md" | "lg" | "full";
/** How tall a control is: 24, 28 or 32 points. Controls of one size line up in a row. */
export type ControlSize = "sm" | "md" | "lg";

/** What a shape the pointer reaches can do. */
interface Pointed extends PointerEvents {
  /** Names it for `now.hover(key)`. */
  key?: Bind<string>;
  /** A drag, made once and given to every shape it drags. */
  drag?: Drag;
  /** Its colour while the pointer is over it. */
  hover?: Bind<Color | undefined>;
  cursor?: "pointer" | "grab" | "crosshair" | "ew-resize" | "ns-resize";
  /** 0 to 1; 1 when left out. */
  opacity?: Bind<Num>;
  /** The point a drag with `snap.grid` puts on the grid: its own `x` and `y` when left out, such as the end of a note for its resize handle. */
  anchor?: Bind<{ x?: number; y?: number }>;
  /** The drags, by `id`, that move it `dx` across and `dy` down while they go on, until its code answers the end: the selected notes of a piano roll. */
  moves?: Bind<readonly string[] | undefined>;
  /** The drags that make it `dx` wider and `dy` taller while they go on, as `moves`: notes being resized. */
  grows?: Bind<readonly string[] | undefined>;
  /** The drags that make it `dx` narrower and `dy` shorter, as `grows`: a clip whose start moves with `moves` too. */
  shrinks?: Bind<readonly string[] | undefined>;
}

/**
 * Something a space draws. Positions and sizes are in the units of the space; `radius` and the
 * `width` of a line are in points.
 */
export type Shape =
  /**
   * Shapes moved together by `x` and `y`, which count from there: one live value for all of
   * them. With `clip`, a box from there out of which they do not show, nor hear the pointer: the
   * notes of a clip end at its end. `moves` and `grows` move it and its box as a shape's do.
   */
  | ({ kind: "group"; x?: Bind<Num>; y?: Bind<Num>; clip?: Bind<{ width: Num; height: Num }> } & Pick<Pointed, "moves" | "grows">)
  | (Pointed &
      (
        | { kind: "rect"; x: Bind<Num>; y: Bind<Num>; width: Bind<Num>; height: Bind<Num>; color: Bind<Paint>; radius?: Bind<number>; border?: Bind<Color>; borderWidth?: Bind<Num> }
        | { kind: "circle"; x: Bind<Num>; y: Bind<Num>; radius: Bind<Num>; color: Bind<Paint> }
        | { kind: "line"; from: Bind<readonly [Num, Num]>; to: Bind<readonly [Num, Num]>; color: Bind<Paint>; width?: Bind<number> }
        /** Along a circle from one angle to another, in turns clockwise from the top: a dial. */
        | { kind: "arc"; x: Bind<Num>; y: Bind<Num>; radius: Bind<number>; from: Bind<Num>; to: Bind<Num>; color: Bind<Paint>; width?: Bind<number> }
        /**
         * Text at `x`, `y`, its top, in a step of the type scale as a `Text`: `variant` is `label`
         * and `tone` that of its variant when left out, and a `color` wins over the tone. A live
         * number shows as a readout in `unit`, see `Unit`, and
         * a tick, such as `now.tick`, as `"position"` (`5.3`), `"time"` (`1:07`) or `"signature"`
         * (the one there). Parts make one line in colours and weights of their own, such as a bar
         * number then a name.
         */
        | { kind: "text"; x: Bind<Num>; y: Bind<Num>; text: Bind<string | Live | readonly TextPart[]>; unit?: Unit | "position" | "time" | "signature" | "beats" | "note"; variant?: TextVariant; tone?: Tone; color?: Bind<Paint>; align?: "left" | "center" | "right" }
        /**
         * The values of `list`, oldest at the left, `low` at the bottom and `high` at the top (-1
         * and 1 when left out), across the whole of its space, or the box `x`, `y`, `width` and
         * `height` say: a line `size` points thick, smooth for a history, or with `band` the
         * lowest to the highest under each point, as a waveform. A line about zero lies on a
         * hairline at zero, and a history that is not full yet is empty at its left. `text` when
         * `color` is left out, as a value is; `green` is for sound that moves. `level` scales
         * them about the middle along it: `[x, factor]` from its left, straight between, so a
         * waveform shows its gain and fades.
         */
        | ({ kind: "wave"; list: Bind<WaveList>; color?: Bind<Paint>; low?: number; high?: number; band?: boolean; size?: number; level?: Bind<readonly (readonly [Num, Num])[]> } & Box)
        /**
         * The notes a `recorder` records so far, `list` its `now.take`, as a clip draws its own: its
         * box, as a wave's, spans the take from its start at the left to the playhead at the right,
         * a line for each note between the lowest pitch at its foot and the highest at its top, 12
         * apart at least.
         */
        | ({ kind: "notes"; list: Bind<TakeList>; color: Bind<Paint> } & Box)
        /**
         * A shape through `points`, in order, filled, or with `width` its outline that many points
         * wide, closed where its last point is its first: a fade's shade, a triangle, a curve. Give
         * them in the binding, `points={() => [...]}`, where TypeScript reads a list as points.
         */
        | { kind: "path"; points: Bind<PathPoints>; color: Bind<Paint>; width?: Bind<number> }
      ));

/** Where a wave or notes are in their space: each edge left out is that edge of what shows of the space. */
interface Box {
  x?: Bind<Num>;
  y?: Bind<Num>;
  width?: Bind<Num>;
  height?: Bind<Num>;
}

/** `[x, y]` a path goes straight to, or curves to as `[cx, cy, x, y]`, pulled towards one control point, or `[c1x, c1y, c2x, c2y, x, y]`, towards two. */
export type PathPoint = readonly [Num, Num] | readonly [Num, Num, Num, Num] | readonly [Num, Num, Num, Num, Num, Num];

/** The points of a `Path`: it starts at an `[x, y]`. A list built in a loop is one of `[x, y]` points. */
export type PathPoints = readonly [readonly [Num, Num], ...PathPoint[]] | readonly (readonly [Num, Num])[];

/** A colour, or one between two by a live value: see `blend`. */
export type Paint = Color | { mix: readonly [Color, Color]; by: Num };

/** `from` at 0, `to` at 1, and between: a light that glows with a level. */
export const blend = (from: Color, to: Color, by: Num): Paint => ({ mix: [from, to], by });

export type ShapeProps<Kind extends Shape["kind"]> = Omit<Extract<Shape, { kind: Kind }>, "kind">;

export function shapeNode(kind: Shape["kind"], props: object, children: Child[]): UiNode {
  return element("shape", { kind, ...props }, children);
}

const shape =
  <Kind extends Exclude<Shape["kind"], "group">>(kind: Kind) =>
  (props: ShapeProps<Kind>): UiNode =>
    shapeNode(kind, props, []);
export const Rect = shape("rect");
export const Circle = shape("circle");
export const Line = shape("line");
export const Arc = shape("arc");
/** A run of a label's text, in its colour and weight unless it names its own; with `border`, in a rounded box of that colour, as a mark in a ruler: give it room with spaces around. */
export type TextPart = { text: string; color?: Color; weight?: "normal" | "medium" | "semibold"; border?: Color };
export const Label = shape("text");
export const Wave = shape("wave");
export const Notes = shape("notes");
export const Path = shape("path");
/** Shapes moved together, see `Shape`. */
export const Group = ({ children, ...props }: ShapeProps<"group"> & { children?: Child }): UiNode => shapeNode("group", props, [children]);

/**
 * A surface that draws shapes in units of its own, such as ticks across and notes down. It
 * scrolls over `extent` and zooms in `zoom` in the window, with no code, and draws only what
 * shows. Without `unit`, a unit is a point. It hears the pointer where no shape that hears it is.
 */
export interface SpaceProps extends PointerEvents {
  /** Names it for `now.space(key)`, and keeps its scroll and zoom when the code changes. One space of a view has it. */
  key?: string;
  /** Its size in points; `grow` takes the room left in its parent. */
  width?: Bind<number>;
  height?: Bind<number>;
  grow?: boolean;
  /** Points per unit at zoom 1. */
  unit?: { x?: number; y?: number };
  /** What it scrolls over, in units; an axis without one does not scroll, and shows from 0. */
  extent?: Bind<{ x?: readonly [number, number]; y?: readonly [number, number] }>;
  /** How far it zooms in and out, as a factor; an axis without one does not zoom. Cmd and scroll, or a pinch. */
  zoom?: { x?: readonly [number, number]; y?: readonly [number, number] };
  /** Where it is scrolled to when it first draws, in units; the window keeps where it is after. */
  scroll?: { x?: number; y?: number };
  /** A range across that fills its width once it is laid out, from `before` points right of its left edge to `after` points left of its right one, as far as `zoom` goes: an editor that opens on what it shows. Once per `key`. */
  fit?: { x: readonly [number, number]; before?: number; after?: number };
  /**
   * The key of a space whose scroll and zoom it shows on an axis, and moves as it scrolls: the
   * ruler and the keys of a piano roll. Give it the same `unit`, `extent` and `zoom` on that axis.
   */
  follow?: { x?: string; y?: string };
  background?: Bind<Color>;
  radius?: Radius | number;
  /**
   * A drag from where no shape that hears the pointer is, such as a marquee. A space whose one
   * drag turns one number, as a knob's or a fader's, is a tab stop the keys turn as the window's knobs.
   */
  drag?: Drag;
  /** Files or a carried value let go on it, where in its units. */
  onDrop?: (drop: Dropped) => void;
  cursor?: Pointed["cursor"];
  children?: Child;
}

export const Space = ({ children, ...props }: SpaceProps): UiNode => element("space", props, [children]);

/**
 * A text field: it shows `value`, and what `value` changes to while it does not have the keys.
 * It calls `onCommit` with its text on enter or when it loses the keys; a click on a button does
 * not take them. Escape shows `value` again and calls `onCancel`.
 */
export interface InputProps {
  key?: string;
  value: Bind<string>;
  placeholder?: string;
  /** In points; 160 when left out. */
  width?: number;
  /** `md` when left out. Read when it is made. */
  size?: ControlSize;
  /** It takes the keys when it is made, with its text selected. */
  focus?: boolean;
  disabled?: Bind<boolean>;
  /** A text area that wraps and grows to `rows` rows: enter adds a line, cmd-enter commits. Read when it is made. */
  rows?: number;
  onCommit?: (text: string) => void;
  onCancel?: () => void;
  /** Each edit, with the text as it is now, such as to filter a list as it is typed. */
  onChange?: (text: string) => void;
}

export const Input = (props: InputProps): UiNode => element("input", { ...props }, []);

/**
 * The card of an instance below this tool's, by its `path` as `now.value` names it, such as
 * `"bass/synth"`: the built-in device's own card, a plugin's with its window, or a tool of the
 * project's view. Its power icon shows `on` with `onPower`, its close icon with `onClose`.
 */
export interface DeviceCardProps {
  key?: string;
  path: Bind<string>;
  on?: Bind<boolean>;
  onPower?: () => void;
  onClose?: () => void;
  /** Its title is a menu: a chevron after it, and a press calls this, such as to put another device in its place. */
  onTitle?: () => void;
  /** What keys a device with a sidechain input, such as a compressor: one or two selects behind its expand. A device without the input shows none. */
  sidechain?: Bind<readonly CardSelect[]>;
}

/** A select of a card, which shows `label` under it: as a `Select`, typed by the values of its options. */
export interface CardSelect<T = unknown> {
  label: string;
  value: T;
  options: readonly SelectOption<T>[];
  onChange(value: T): void;
}

export const DeviceCard = ({ path, sidechain, ...props }: DeviceCardProps): UiNode => {
  if (sidechain === undefined) return element("card", { instance: path, ...props }, []);
  const selects = () => (typeof sidechain === "function" ? sidechain() : sidechain);
  // The window shows each select by the labels of its options, and tells a pick by its places.
  const shown = () =>
    selects().map(({ label, value, options }) => ({
      label,
      value: options.find((option) => option.value === value)?.label ?? String(value),
      options: options.map((option) => option.label),
    }));
  const onSidechain = (select: number, option: number) => {
    const picked = untrack(selects)[select];
    const chosen = picked?.options[option];
    if (picked && chosen) picked.onChange(chosen.value);
  };
  return element("card", { instance: path, ...props, sidechain: shown, onSidechain }, []);
};

export type IconName =
  | "arrow-left" | "arrow-right" | "arrow-up" | "audio-lines" | "automation" | "bot" | "check"
  | "chevron-down" | "chevron-left" | "chevron-right" | "chevron-up" | "circle" | "circle-alert"
  | "circle-check" | "claude" | "copy" | "device-analyzer" | "device-compressor" | "device-delay"
  | "device-drum-pad" | "device-eq" | "device-filter" | "device-gate" | "device-limiter"
  | "device-modulation" | "device-plugin" | "device-reverb" | "device-sampler" | "device-saturator"
  | "device-synth" | "device-utility" | "device-wavetable" | "ellipsis" | "eq-bell" | "eq-high-cut"
  | "eq-high-shelf" | "eq-low-cut" | "eq-low-shelf" | "eq-notch" | "eye" | "eye-off" | "folder" | "info"
  | "keyboard-music" | "loader-circle" | "lock" | "lock-open" | "metronome" | "mic" | "minus" | "music"
  | "panel-left" | "pause" | "pencil" | "piano" | "play" | "plus" | "power" | "redo-2" | "search" | "section"
  | "send" | "settings" | "shield" | "skip-back" | "skip-forward" | "sliders-horizontal" | "sparkles"
  | "square" | "tempo-change" | "time-signature" | "trash-2" | "triangle-alert" | "undo-2" | "volume-2"
  | "volume-x" | "x" | "zap";

/** An icon of the app, 16 points when `size` is left out, in the text's colour unless `color` says. */
export const Icon = (props: { name: Bind<IconName>; size?: number; color?: Bind<Color> }): UiNode => element("icon", { ...props }, []);

/** The tooltip of a button or a toggle, and the keys that do it too, drawn in it. */
type Tip = { tooltip?: Bind<string>; shortcut?: never } | { tooltip: Bind<string>; shortcut: Shortcut };

/** What a switch shows: a letter or a name (`label`), an `icon` with a `count` after it, or a `dot`. */
type Face =
  | { label: Bind<string>; icon?: never; count?: never; dot?: never }
  | { icon: Bind<IconName>; count?: Bind<number>; label?: never; dot?: never }
  | { dot: true; label?: never; icon?: never; count?: never };

/**
 * A switch of the app, coloured while it is on, `color` when given. On a boolean `field` it
 * says the field's label unless it has another face, and a click turns the field as one undo
 * step: `<Toggle field="hold" />`. Or it shows `on`, and its code turns it in `onClick`.
 */
export type ToggleProps = (
  | ({ field: string; on?: never } & (Face | { [Key in "label" | "icon" | "count" | "dot"]?: never }))
  | ({ on: Bind<boolean>; field?: never } & Face & PointerEvents)
) & Tip & {
  key?: string;
  color?: Bind<Color>;
  /** `md` when left out. */
  size?: ControlSize;
  disabled?: Bind<boolean>;
};

export function Toggle(props: ToggleProps): UiNode {
  if (props.field === undefined) return element("toggle", { ...props }, []);
  const { field, ...look } = props;
  const [{ get, set }, name] = [valueOf<boolean>({ field }), labelOf(field)];
  const turn = () => {
    const on = untrack(get);
    set(!on, `Turn ${name.toLowerCase()} ${on ? "off" : "on"}`);
  };
  const named = look.icon === undefined && look.dot === undefined ? { label: look.label ?? name } : {};
  return element("toggle", { key: `toggle-${field}`, ...look, ...named, on: get, onClick: turn }, []);
}

/** A button of the app: a `label`, an `icon` or both, as the window's own buttons look. */
export type ButtonProps = PointerEvents & Tip & {
  key?: string;
  label?: Bind<string>;
  icon?: Bind<IconName>;
  /** `primary` when left out. `solid`, `subtle` and `ghost` take `color` as their accent. */
  variant?: Bind<"primary" | "subtle" | "ghost" | "solid">;
  color?: Bind<Color>;
  /** `md` when left out. */
  size?: ControlSize;
  /** A pill, round at its ends. */
  rounded?: boolean;
  disabled?: Bind<boolean>;
};

export const Button = (props: ButtonProps): UiNode => element("button", { ...props }, []);

/** A step of the type scale: `display` is 24 points, `title` 16 medium, `heading` 14 medium, `body` 14 and `label` 12. */
export type TextVariant = "display" | "title" | "heading" | "body" | "label";
/** How bright a text is: `text`, `secondary` as the label of a control, `muted` as a hint or a unit. */
export type Tone = "text" | "secondary" | "muted";

export interface TextProps {
  /** Names it for tests, `div-<key>`. */
  key?: string;
  /** `body` when left out. */
  variant?: Bind<TextVariant>;
  /** `secondary` for a `label` and `text` for the others when left out. A `color` of its `style` wins: a hue, or `text-inverse` on a bright fill. */
  tone?: Bind<Tone>;
  /** Where it sits and how it is cut: `grow`, `width`, `textAlign`, `truncate`. */
  style?: Bind<Style>;
  tooltip?: Bind<string>;
  children?: Child;
}

/**
 * Text in a size of the type scale. Its numbers have digits of one width, so a number that
 * changes moves nothing after it. `<Text variant="label" tone="muted">{() => `${state.notes.length} notes`}</Text>`
 */
export const Text = ({ children, variant = "body", ...props }: TextProps): UiNode => element("div", { variant, ...props }, [children]);

export interface RowProps {
  /** Names it for tests, `div-<key>`. */
  key?: string;
  /** The room between its parts: `md` when left out. */
  gap?: Spacing;
  /** Where its parts sit up and down: in its middle when left out, or as tall as it when it grows. */
  align?: Style["align"];
  /** Where its parts sit along it. */
  justify?: Style["justify"];
  /** Takes the room left in its parent. */
  grow?: boolean;
  children?: Child;
}

/** Parts side by side with one room between them: the knobs under a display, the panels of a page. */
export const Row = ({ key, children, gap = "md", grow, align = grow ? undefined : "center", justify }: RowProps): UiNode =>
  element("div", { key, style: { direction: "row", gap, align, justify, grow } }, [children]);

/** How a `div` lays out and looks. A gap, a padding and a radius name a step of their scale; a number is points, for what the scale cannot say. */
export interface Style {
  /** How children line up. A div is a column unless this says `row`. */
  direction?: "row" | "column";
  gap?: Spacing | number;
  padding?: Spacing | number;
  paddingX?: Spacing | number;
  paddingY?: Spacing | number;
  paddingLeft?: Spacing | number;
  paddingRight?: Spacing | number;
  paddingTop?: Spacing | number;
  paddingBottom?: Spacing | number;
  width?: number;
  height?: number;
  minWidth?: number;
  maxWidth?: number;
  minHeight?: number;
  maxHeight?: number;
  /** Takes the room left in its parent, and no more: a long list in it scrolls. */
  grow?: boolean;
  /** Out of its parent's flow, over its siblings, this far from its parent's edges: a badge, an overlay. */
  absolute?: { top?: number; left?: number; right?: number; bottom?: number };
  /** Its children go on to a next line where a row is full. */
  wrap?: boolean;
  /** Where children sit across the direction. */
  align?: "start" | "center" | "end";
  /** Where children sit along the direction. */
  justify?: "start" | "center" | "end" | "between";
  background?: Color;
  color?: Color;
  /** The weight of its text, for the face of a control, which is medium at any size. */
  weight?: "normal" | "medium" | "semibold";
  radius?: Radius | number;
  borderColor?: Color;
  borderWidth?: number;
  /** Its children scroll along this axis where they do not fit its size, as a long rack does. */
  scroll?: "x" | "y";
  textAlign?: "start" | "center" | "end";
  /** 0 to 1. */
  opacity?: number;
  /** The shadow of the app's menus. */
  shadow?: boolean;
  /** Its children do not show outside it. */
  clip?: boolean;
  /** Its look while the pointer is over it, while a press on it is held, and while its div is `disabled` (40% opacity when left out). */
  hover?: Look;
  pressed?: Look;
  disabled?: Look;
  /** It never shrinks below what it needs, in a row too narrow. */
  keep?: boolean;
  /** One line, cut with an ellipsis where it does not fit. A text it cuts shows whole on hover, unless the div has a `tooltip`. */
  truncate?: boolean;
}

type LookKey = "background" | "color" | "borderColor" | "opacity";
/** What a look changes of a div's `Style`; a `Style` with more in it is no look. */
export type Look = Pick<Style, LookKey> & { [Key in Exclude<keyof Style, LookKey>]?: never };

interface DivProps extends PointerEvents {
  /** Names it for tests, `div-<key>`, and keeps its scroll when the code changes. One div of a view has it: a row's comes from its item. */
  key?: string;
  style?: Bind<Style>;
  /** It hears no press, release, click, drag or drop and shows its `disabled` look; its children still hear theirs, and it still scrolls. */
  disabled?: Bind<boolean>;
  cursor?: Pointed["cursor"];
  tooltip?: Bind<string>;
  /** With an `onClick`: Tab reaches it, its ring shows, and enter clicks it, as a button's, for a div that does what a button does, such as a track's head. The menu key, or shift-F10, presses it as the right button does, for its menu. With `keys` alone, Tab reaches it for them. */
  tabStop?: boolean;
  /** The keys it takes while the focus is in it, which `onKey` hears and its page does not: `["up", "down", "enter"]`. With `tabStop`, it holds the focus itself. */
  keys?: readonly KeyName[];
  onKey?: (key: Key) => void;
  /** The pointer came over it, `true`, or left it, `false`, also over its children. */
  onHover?: (over: boolean) => void;
  /** A drag of it carries this to a node that hears drops, such as another row of a list it reorders. */
  carry?: string;
  /** Files from the system, or what a dragged `div` carries, let go on it. */
  onDrop?: (drop: Dropped) => void;
  /**
   * The key of a space whose scroll its children move with, on an axis, cut off at its edges:
   * the heads of the rows of a timeline, which scroll with it. Rows as tall as the space's unit.
   */
  follow?: { x?: string; y?: string };
  children?: Child;
}

type Component<Props> = (props: Props) => UiNode | Region;

/** Makes the nodes of JSX. `tsconfig.json` next to this file points JSX here. */
export function h<Props>(type: "div" | Component<Props>, props: (Props & DivProps) | null, ...children: Child[]): UiNode {
  if (typeof type === "function") {
    // One child as it is, such as the row of a `For`; a component without children gets none.
    const given = children.length === 0 ? {} : { children: children.length === 1 ? children[0] : children };
    return setup(() => type({ ...props, ...given } as Props)) as UiNode;
  }
  return element("div", { ...props }, children);
}

export namespace h {
  export namespace JSX {
    export type Element = UiNode | Region;
    export interface IntrinsicElements {
      div: DivProps;
    }
    export interface ElementChildrenAttribute {
      children: {};
    }
  }
}
