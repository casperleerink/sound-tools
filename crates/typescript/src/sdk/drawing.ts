// What a card draws.

import { SelectOption } from "./components";
import { Unit } from "./fields";
import { Drag, Dropped, Live, Num, PointerEvents, TakeList, WaveList } from "./live";
import { Child, element, Region, UiNode } from "./nodes";
import { Bind, setup, untrack } from "./reactivity";
import type { Input as InputOfTool } from "./fields";
import type { Notes as NotesOfSound } from "./sound";

/** `Input` is also the type of an input of a tool, `Notes` that of notes a sound sends. */
export type Input = InputOfTool;
export type Notes = NotesOfSound;

/**
 * A colour of the app's theme, as the window draws its own controls. Gray goes from the window's
 * background, `gray-50`, to text, `gray-950`; a card is `gray-200`. `alpha` is white, so
 * `alpha/10` lifts a surface off whatever is under it.
 */
export type ThemeColor =
  | "gray-50" | "gray-100" | "gray-200" | "gray-300" | "gray-400" | "gray-500" | "gray-600" | "gray-700" | "gray-800" | "gray-900" | "gray-950"
  | "alpha" | "blue" | "sapphire" | "sky" | "teal" | "green" | "yellow" | "peach" | "red" | "maroon" | "mauve" | "pink" | "lavender" | "rosewater" | "flamingo";
/** `#rrggbb`, `#rrggbbaa`, a colour of the theme, or one at an opacity in percent: `"alpha/10"`, `"#a9b1ff/70"`. */
export type Color = `#${string}` | ThemeColor | `${ThemeColor}/${number}`;

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
  /**
   * The drags it moves with, by their `id`: while one goes on, it is `dx` across and `dy` down
   * from its place, in the window, until its code answers the end. The selected notes of a piano
   * roll: `moves={() => (picked() ? ["move"] : undefined)}`, one small change as the selection does.
   */
  moves?: Bind<readonly string[] | undefined>;
  /** The drags that make it `dx` wider and `dy` taller while they go on, as `moves`: notes being resized. */
  grows?: Bind<readonly string[] | undefined>;
  /** The drags that make it `dx` narrower and `dy` shorter, as `grows`: a clip whose start moves with `moves` too. */
  shrinks?: Bind<readonly string[] | undefined>;
}

/**
 * Something a space draws. Positions and sizes are in the units of the space; `radius`, `width`
 * of a line and `size` of text are in points.
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
         * Text at `x`, `y`, its top. A live number shows as a readout in `unit`, and a tick, such as
         * `now.tick`, as `"position"` (`5.3`), `"time"` (`1:07`) or `"signature"` (the one there).
         * Parts make one line in colours and weights of their own, such as a bar number then a name.
         */
        | { kind: "text"; x: Bind<Num>; y: Bind<Num>; text: Bind<string | Live | readonly TextPart[]>; unit?: Unit | "pan" | "position" | "time" | "signature" | "beats" | "note"; color?: Bind<Paint>; size?: number; align?: "left" | "center" | "right" }
        /**
         * The values of `list` across its box, oldest at the left, `low` at its bottom and `high` at
         * its top (-1 and 1 when left out): a line `size` points thick, or with `band` the lowest to
         * the highest under each point, as a waveform. `level` scales them about the middle along
         * it: `[x, factor]` from its left, straight between, so a waveform shows its gain and fades.
         */
        | { kind: "wave"; x: Bind<Num>; y: Bind<Num>; width: Bind<Num>; height: Bind<Num>; list: Bind<WaveList>; color: Bind<Paint>; low?: number; high?: number; band?: boolean; size?: number; level?: Bind<readonly (readonly [Num, Num])[]> }
        /**
         * The notes a `recorder` records so far, `list` its `now.take`, as a clip draws its own: its
         * box spans the take from its start at the left to the playhead at the right, a line for
         * each note between the lowest pitch at its foot and the highest at its top, 12 apart at least.
         */
        | { kind: "notes"; x: Bind<Num>; y: Bind<Num>; width: Bind<Num>; height: Bind<Num>; list: Bind<TakeList>; color: Bind<Paint> }
        /**
         * A shape through `points`, in order, filled, or with `width` its outline that many points
         * wide, closed where its last point is its first: a fade's shade, a triangle, a curve. Give
         * them in the binding, `points={() => [...]}`, where TypeScript reads a list as points.
         */
        | { kind: "path"; points: Bind<readonly [readonly [Num, Num], ...PathPoint[]]>; color: Bind<Paint>; width?: Bind<number> }
      ));

/** `[x, y]` a path goes straight to, or curves to as `[cx, cy, x, y]`, pulled towards one control point, or `[c1x, c1y, c2x, c2y, x, y]`, towards two. */
export type PathPoint = readonly [Num, Num] | readonly [Num, Num, Num, Num] | readonly [Num, Num, Num, Num, Num, Num];

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
  /** Names it for `now.space(key)`, and keeps its scroll and zoom. */
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
  radius?: number;
  /** A drag from where no shape that hears the pointer is, such as a marquee. */
  drag?: Drag;
  /** Files or a carried value let go on it, where in its units. */
  onDrop?: (drop: Dropped) => void;
  cursor?: Pointed["cursor"];
  children?: Child;
}

/** A space, see `SpaceProps`. */
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

/** A text field, see `InputProps`. */
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
  /**
   * What keys a device with a sidechain input, such as a compressor or a gate: one or two
   * selects behind its expand, as the window's cards had them. A device without the input
   * shows none.
   */
  sidechain?: Bind<readonly CardSelect[]>;
}

/** A select of a card, which shows `label` under it: as a `Select`, typed by the values of its options. */
export interface CardSelect<T = unknown> {
  label: string;
  value: T;
  options: readonly SelectOption<T>[];
  onChange(value: T): void;
}

/** The card of an instance, see `DeviceCardProps`. */
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

/** An icon of the app, by its name. */
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

/**
 * A stereo meter as the window draws its own: bars that fall, a peak line that holds, a clip light
 * a click clears. Of the peaks `of` names as `now.peak` does, of the channels of the audio input
 * `input` names (one shows on both bars), or of what the device plays. Up the side unless
 * `horizontal`; `length` along its bars, in points.
 */
export const PeakMeter = (props: { key?: string; of?: string; input?: readonly number[]; horizontal?: boolean; length?: number }): UiNode =>
  element("meter", { ...props }, []);

/**
 * A switch of the app: a letter (`label`), an `icon` with a `count` after it, or a `dot`.
 * Coloured while `on`, `color` when given; its code turns it in `onClick`.
 */
export type ToggleProps = (
  | { label: Bind<string>; icon?: never; count?: never; dot?: never }
  | { icon: Bind<IconName>; count?: Bind<number>; label?: never; dot?: never }
  | { dot: true; label?: never; icon?: never; count?: never }
) & PointerEvents & {
  key?: string;
  on: Bind<boolean>;
  color?: Bind<Color>;
  disabled?: Bind<boolean>;
  tooltip?: Bind<string>;
};

/** A switch, see `ToggleProps`. */
export const Toggle = (props: ToggleProps): UiNode => element("toggle", { ...props }, []);

/** A button of the app: a `label`, an `icon` or both, as the window's own buttons look. */
export interface ButtonProps extends PointerEvents {
  key?: string;
  label?: Bind<string>;
  icon?: Bind<IconName>;
  /** `primary` when left out. `solid`, `subtle` and `ghost` take `color` as their accent. */
  variant?: Bind<"primary" | "subtle" | "ghost" | "solid">;
  color?: Bind<Color>;
  /** 24, 28 or 32 points tall; `md` when left out. */
  size?: "xs" | "sm" | "md";
  /** A pill, round at its ends. */
  rounded?: boolean;
  disabled?: Bind<boolean>;
  /** Quieter than its look, 0 to 1: a close button that stays out of the way. */
  opacity?: number;
  tooltip?: Bind<string>;
}

/** A button, see `ButtonProps`. */
export const Button = (props: ButtonProps): UiNode => element("button", { ...props }, []);

/**
 * Shows its children in the window's title bar, in the middle of the room right of the project
 * menu, when this tool's view is the window's: the transport of a song. Anywhere else it shows
 * nothing.
 */
export const TitleBar = ({ children, style }: { children?: Child; style?: Bind<Style> }): UiNode =>
  element("div", { titleBar: true, style }, [children]);

/** How a `div` lays out and looks. Numbers are points. */
export interface Style {
  /** How children line up. A div is a column unless this says `row`. */
  direction?: "row" | "column";
  gap?: number;
  padding?: number;
  paddingX?: number;
  paddingY?: number;
  paddingLeft?: number;
  paddingRight?: number;
  paddingTop?: number;
  paddingBottom?: number;
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
  fontSize?: number;
  /** How far apart the lines of its text are, in points: 14 under 12-point text, as a card's note. */
  lineHeight?: number;
  radius?: number;
  borderColor?: Color;
  borderWidth?: number;
  /** Its children scroll along this axis where they do not fit its size, as a long rack does. */
  scroll?: "x" | "y";
  /** `tabular`: digits of one width, so a number that changes does not move what follows. */
  font?: "tabular";
  weight?: "normal" | "medium" | "semibold";
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
  /** One line, cut with an ellipsis where it does not fit. */
  truncate?: boolean;
}

type LookKey = "background" | "color" | "borderColor" | "opacity";
/** What a look changes of a div's `Style`; a `Style` with more in it is no look. */
export type Look = Pick<Style, LookKey> & { [Key in Exclude<keyof Style, LookKey>]?: never };

interface DivProps extends PointerEvents {
  key?: string;
  style?: Bind<Style>;
  /** It hears no press, release, click, drag or drop and shows its `disabled` look; its children still hear theirs, and it still scrolls. */
  disabled?: Bind<boolean>;
  cursor?: Pointed["cursor"];
  tooltip?: Bind<string>;
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
