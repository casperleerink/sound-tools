// Components made of the blocks above.

import { Arc, Circle, ControlSize, h, Icon, IconName, Input, Label, Line, Rect, Space, Text } from "./drawing";
import { Field, Fields, NumberField, ObjectItem, Unit } from "./fields";
import { Drawing, host } from "./host";
import { Drag, DragTurns, heldByLane, Live, now, Num, Press, splitPath, touched, Turn } from "./live";
import { asCell, currentCard, element, ownLines, Show, UiNode } from "./nodes";
import { Accessor, Bind, signal, untrack } from "./reactivity";
import { builtInNumbers } from "./records";

/** The view that draws now, for the components below, which read its record and fields. */
function drawing(): Drawing {
  return currentCard().drawn;
}

/** A value code keeps, such as in `memory`: a control shows it, and `onChange` hears each value a drag or a key gives it, as it goes. */
export type Kept<T> = { value: Bind<T>; onChange: (value: T) => void };

/** `A` or `B`, never a mix: each leaves out the keys of the other. */
type Either<A, B> = (A & { [K in Exclude<keyof B, keyof A>]?: never }) | (B & { [K in Exclude<keyof A, keyof B>]?: never });

/**
 * What a control shows and changes: a `field` as `now.value` names it, such as `"cutoff"`,
 * `"bass/mix.gain_db"` or `"kit.pads.36.volume_db"`, whose change is one undo step; or a value
 * code keeps, see `Kept`. A control on a field takes its range, default, unit, label and
 * options from the field, and each can still be said; on what code keeps they are said.
 */
export type ValueProps<T> = Either<{ field: string }, Kept<T>>;

const read = <V,>(bound: Bind<V>): V => (typeof bound === "function" ? (bound as Accessor<V>)() : bound);

const within = (position: number) => Math.min(Math.max(position, 0), 1);

let controls = 0;
/** The key of a control on what code keeps, when its props name none. */
const controlId = (kind: string) => `${kind}-${++controls}`;

/** The parts of where a record holds a value: `bands[0].gain_db` is `bands`, `0` and `gain_db`. */
const partsOf = (name: string) => name.split(/[.[\]]/).filter((part) => part !== "");

/** A field as a control finds it: of a tool of the project, or a number of a built-in one, whose volume is on the fader's scale. */
type Found = Field & { builtIn?: { fader: boolean } };

/** The field at `parts` in `fields`: an item of a list by its place, a field of an item by its name. */
function fieldIn(fields: Fields, parts: readonly string[]): Field | undefined {
  let at: Field | ObjectItem | undefined = { kind: "object", fields };
  for (const part of parts) {
    if (at?.kind === "object") at = at.fields[part];
    else if (at?.kind === "list" && /^\d+$/.test(part)) at = at.item;
    else return undefined;
  }
  return at?.kind === "object" ? undefined : at;
}

/** The units a name of a built-in number ends in, as `cutoff_hz`. */
const NAMED_UNITS = ["hz", "db", "ms", "seconds", "semitones", "cents", "octaves"] as const;

/** A number of a built-in tool as a field: its range, and the unit its name says. */
function builtInNumber(tool: string, parts: readonly string[]): Found | undefined {
  const fits = (pattern: string) => {
    const each = pattern.split(".");
    return each.length === parts.length && each.every((part, at) => part === "*" || part === parts[at]);
  };
  const found = Object.entries(builtInNumbers[tool] ?? {}).find(([pattern]) => fits(pattern))?.[1];
  if (!found) return undefined;
  const { min = -Infinity, max = Infinity, scale } = found;
  const last = parts.at(-1) ?? "";
  const unit = NAMED_UNITS.find((unit) => last.endsWith(`_${unit}`)) ?? (last === "pan" ? "pan" : min === 0 && max === 1 ? "percent" : undefined);
  return { kind: "number", min, max, default: found.default, unit: scale === "log" || unit !== "hz" ? unit : undefined, builtIn: { fader: scale === "fader" } };
}

/**
 * The tool of the child at `path` below `tool`, as each parent's `children` names it, by its name
 * or the longest pattern it starts as. `undefined` for one a `takes` slot holds, or one only a
 * record names.
 */
function toolAt(tool: string, path: string): string | undefined {
  let at: string | undefined = tool;
  for (const name of path.split("/")) {
    const children: Record<string, string> = (at === undefined ? undefined : host.tools.get(at)?.children) ?? {};
    const patterns = Object.keys(children).filter((pattern) => pattern.endsWith("*") && name.startsWith(pattern.slice(0, -1)));
    at = children[name] ?? children[patterns.sort((one, other) => other.length - one.length)[0] ?? ""];
  }
  return at?.startsWith("takes:") ? undefined : at;
}

/**
 * The field `path` names, see `splitPath`: of the tool that draws, or of the tool of a child,
 * built-in or of this project. `undefined` where no tool tells it: a parameter of a plugin, or a
 * child whose tool only its record says, before there is one.
 */
function fieldOf(path: string): Found | undefined {
  const { fields, state, children, tool: own } = drawing();
  const { child, name } = splitPath(path);
  const parts = partsOf(name);
  if (child === undefined) return fieldIn(fields, parts) ?? numberInput(own, parts);
  const tool = toolAt(own, child) ?? untrack(() => recordAt({ state, children }, child)?.tool);
  if (tool === undefined) return undefined;
  const spec = host.tools.get(tool);
  return spec ? fieldIn(spec.state, parts) : builtInNumber(tool, parts);
}

/** A number input of `tool` by its name: a control turns it as it turns a field. */
function numberInput(tool: string, parts: readonly string[]): Field | undefined {
  const [name, ...deeper] = parts;
  const input = name === undefined || deeper.length > 0 ? undefined : host.tools.get(tool)?.inputs?.[name];
  return input?.kind === "number" ? input : undefined;
}

/** `cutoff_hz` is `Cutoff`: a name in words, without the unit it ends in. */
const named = (name: string) => words(name.replace(new RegExp(`_(${NAMED_UNITS.join("|")}|percent)$`), ""));

/** What a control calls the field at `path`: its own label, else its name in words. */
export const labelOf = (path: string, field = fieldOf(path)) => field?.label ?? named(partsOf(splitPath(path).name).at(-1) ?? path);

/** The label a control on a field has as a cell of `Controls`; one on what code keeps has none. */
const cellLabel = (props: { field?: string }) => (props.field === undefined ? undefined : labelOf(props.field));

/** A field, or what code keeps, as a control reads and sets it. A field's change is one undo step of `label`. */
export function valueOf<T>(props: ValueProps<T>): { get: () => T; set: (value: T, label?: string) => void } {
  if (props.field === undefined) {
    const { value, onChange } = props;
    return { get: () => read(value), set: (value) => onChange(value) };
  }
  const { state, children, update } = drawing();
  const { child, name } = splitPath(props.field);
  const [parts, change] = [partsOf(name), `Change ${labelOf(props.field).toLowerCase()}`];
  type Held = Record<string, unknown> | undefined;
  return {
    get: () => parts.reduce<unknown>((at, part) => (at as Held)?.[part], recordAt({ state, children }, child)?.state) as T,
    set: (value, label) =>
      update(label ?? change, (part) => {
        let at: Held = recordAt(part, child)?.state;
        for (const each of parts.slice(0, -1)) {
          // An object the record leaves out is made, such as a pad of a kit; an item of a list is not.
          if (at && !Array.isArray(at)) at[each] ??= {};
          at = at?.[each] as Held;
        }
        if (at) at[parts.at(-1) ?? name] = value;
      }),
  };
}

/** The arc of a knob runs from -135 to +135 degrees, in turns. */
const SWEEP = 0.75;

/** Points of a drag for the whole range of a knob, as the window's. */
const TRAVEL = 200;

/** How a knob spreads a number from `min` to `max`, as the window's knobs: on a log scale for `"hz"`. */
const knobTravel = (min: number, max: number, unit?: Unit): Travel => ({ min, max, scale: unit === "hz" && min > 0 ? "log" : "linear" });

/** The fader's own travel, whatever its ends. */
const FADER_TRAVEL: Travel = { min: 0, max: 0, scale: "fader" };

/** Where a live `value` sits along `travel`, as `positionOf` puts a number, worked out in the window. */
function livePosition({ min, max, scale }: Travel, value: Live): Live {
  if (scale === "log") return value.over(min).decibels().over(20 * Math.log10(max / min));
  if (scale === "fader") return Live.of(10).pow(value.over(FADER.decade)).times(FADER.unity).min(1);
  return value.minus(min).over(max - min);
}

/** The range of a number control and where a double click puts it. */
type Range = { min: number; max: number; default: number };

/** A number control: on a `field` its range and default are the field's unless said; on what code keeps they are said. */
type NumberValue = Either<{ field: string } & Partial<Range>, Kept<number> & Range>;

/** What a number control is on, with what its field tells where its props do not say. */
function numberOf(kind: string, props: { field?: string; min?: number; max?: number; default?: number; unit?: Unit; label?: string }) {
  const found = props.field === undefined ? undefined : fieldOf(props.field);
  const field = found?.kind === "number" ? (found as NumberField & Found) : undefined;
  const [min, max] = [props.min ?? field?.min, props.max ?? field?.max];
  const unit = props.unit ?? field?.unit;
  const label = props.label ?? (props.field === undefined ? undefined : labelOf(props.field, found));
  const fader = field?.builtIn?.fader === true && props.min === undefined;
  const ranged = min !== undefined && max !== undefined && Number.isFinite(min) && Number.isFinite(max) && !fader;
  const turns: Omit<DragTurns, "value" | "onTurn"> = {
    ...(ranged ? { min, max, unit } : { fader }),
    // A whole number turns in whole steps over the same travel.
    ...(ranged && field?.integer ? { step: 1, per: TRAVEL / Math.max(max - min, 1) } : {}),
    // A field of the project that names no default starts at its min; a built-in number says none.
    default: props.default ?? field?.default ?? (field?.builtIn ? undefined : field?.min),
    label: label === undefined ? undefined : `Change ${label.toLowerCase()}`,
  };
  return {
    field,
    unit,
    label,
    /** `undefined` for a field whose range only the window knows: a fader's volume, or a number no tool describes. */
    travel: ranged ? knobTravel(min, max, unit) : fader ? FADER_TRAVEL : undefined,
    turns,
    /** Why `kind` cannot draw: it needs a range, and neither its props nor its field give one. */
    unranged: () => new Error(`${kind}: ${JSON.stringify(props.field)} is no number field with a range. Name a number field, or give it min and max.`),
  };
}

/** A value to send the window: -inf, a silent volume, as the lowest it takes. */
const finite = (value: number) => (Number.isFinite(value) ? value : value < 0 ? -1000 : 1000);

/** The value `moved` of the way along `travel` from `from`, with three digits that count, as the window gives a field's. */
function movedAlong(travel: Travel, from: number, moved: number): number {
  const value = Number(valueAt(travel, within(positionOf(travel, from) + moved)).toPrecision(3));
  return travel.scale === "fader" ? value : Math.min(Math.max(value, Math.min(travel.min, travel.max)), Math.max(travel.min, travel.max));
}

/**
 * What code keeps as the window's drags and keys turn it. Only code can read it, so the press
 * notes where it is and each move is worked out from there: `to` gives the value `moved` away
 * from one.
 */
function keptTurns(get: () => number, set: (value: number) => void, to: (from: number, moved: number) => number, reset?: number) {
  let pressed = 0;
  return {
    onPress: () => {
      pressed = untrack(get);
    },
    onTurn: (turn: Turn) => {
      if ("reset" in turn) return reset === undefined ? undefined : set(reset);
      if ("moved" in turn) set(to(turn.held ? pressed : untrack(get), turn.moved));
    },
  };
}

/**
 * What a knob or a fader shows and the drag that turns it, `along` an axis. The window measures
 * the drag and the keys of both a field and what code keeps, so they behave as one: a field is
 * changed there, and `onChange` hears each value of what code keeps as it goes. `held` is 1
 * while a lane or a cable moves a field: the window then lets no drag or key turn it.
 */
function turned(kind: string, props: Either<{ field: string }, Kept<number>> & { key?: string }, travel: Travel | undefined, turns: Omit<DragTurns, "value" | "onTurn">, along: "x" | "y") {
  if (props.field !== undefined) {
    const name = props.field;
    const value = now.value(name);
    const fraction = travel ? livePosition(travel, value).clamp(0, 1) : now.value(name, { fraction: true });
    return { key: props.key ?? `${kind}-${name}`, value, fraction, held: heldByLane(name), drag: { [along]: { value: name, ...turns } } as Drag, onPress: undefined };
  }
  const { value, onChange } = props;
  const at = travel ?? FADER_TRAVEL;
  const { onPress, onTurn } = keptTurns(() => read(value), onChange, (from, moved) => movedAlong(at, from, moved), turns.default);
  return {
    key: props.key ?? controlId(kind),
    value: () => Live.of(finite(read(value))),
    fraction: () => Live.of(within(positionOf(at, read(value)))),
    held: undefined,
    drag: { [along]: { onTurn, ...turns } } as Drag,
    onPress,
  };
}

/**
 * `quiet` keeps a number out of sight until it matters: it shows while the pointer or the keys
 * are on a control of its value, while a drag holds one, and while it moves and a second after.
 * So a row of knobs at rest says no number, and a fader shows the quiet number beside it.
 */
export type Quiet = { quiet?: Bind<boolean> };

/** How much the number of the control on `field`, or of the `key` of one on what code keeps, shows, see `Quiet`. Only the window sees a field move. */
function quietly(quiet: Bind<boolean> | undefined, { field, key }: { field?: string; key: string }): Bind<Num> | undefined {
  if (quiet === undefined) return undefined;
  const value = field === undefined ? undefined : now.value(field);
  const on = touched(field ?? key);
  const shown = (value ? on.max(value.smooth(60).minus(value).abs().gt(0.001)) : on).hold(1);
  return () => (read(quiet) ? shown : 1);
}

/** The dot of a control whose field a lane or a cable moves, as the window's knobs have: 4 points, in the grey of a label. */
const heldMark = (held: Live | undefined, x: number, y: number): UiNode[] => (held ? [Circle({ x, y, radius: 2, color: "text-secondary", opacity: held })] : []);

export type KnobProps = NumberValue & Quiet & {
  key?: string;
  /** The field's own label, else its name in words, when left out. It names its undo step too. */
  label?: string;
  /** The field's own when left out. */
  unit?: Unit;
  bipolar?: boolean;
  /** How much text it shows: its label and its value (`"all"`, when left out), as a cell of `Controls`, its home; its `"value"` alone, for a knob of a row that a line over the rows names once; or `"none"`, a 28-point dial that fits a row of controls. */
  text?: "all" | "value" | "none";
};

/** A cell of `Controls` and the room of a control at its top, as the window's: a knob is one cell. */
const [CELL_WIDTH, CELL_CONTROL] = [64, 36];

/**
 * A knob, as the app's knobs look: `<Knob field="cutoff" />`, or on what code keeps, see
 * `ValueProps`. Both turn as they are dragged, are a tab stop the arrows step, and go to
 * `default` on a double click or backspace. Enter, or a click on its value, types the value as
 * it reads: `F#2`, `440`, `50`. A field shows what plays: while a lane or a cable moves it, the
 * knob follows that, with a dot and a tooltip, and no drag turns it.
 * `bipolar` draws its arc from the top, as a pan's. `quiet` hides its value, see `Quiet`.
 */
export function Knob(props: KnobProps): UiNode {
  const { field, label, unit, travel, turns, unranged } = numberOf("Knob", props);
  if (!travel || travel.scale === "fader") throw unranged();
  const { key, value, fraction, held, drag, onPress } = turned("knob", props, travel, turns, "y");
  const turn = (): Live => read(fraction).times(SWEEP).minus(SWEEP / 2);
  const dial = props.text === "none";
  const [centreX, centreY, radius, cap] = dial ? [14, 14, 11, 7] : [CELL_WIDTH / 2, CELL_CONTROL / 2, 16.75, 10.5];
  const along = (distance: number) => () => {
    const radians = turn().times(2 * Math.PI);
    return [radians.sin().times(distance).plus(centreX), radians.cos().times(-distance).plus(centreY)] as const;
  };
  const said = label !== undefined && (props.text ?? "all") === "all";
  const [labelY, valueY] = said ? [38, 54] : [0, 38];
  // Made with the knob each time it shows again, after its value was typed.
  const texts = () => dial ? [] : [
    ...(said ? [Label({ x: centreX, y: labelY, text: label, color: "text-secondary", align: "center" })] : []),
    Label({ x: centreX, y: valueY, text: value, unit, color: "text", align: "center", opacity: quietly(props.quiet, { field: props.field, key }) }),
  ];
  const [width, height] = dial ? [28, 28] : [CELL_WIDTH, valueY + 18];
  const [typing, type] = signal(false);
  const knob = (): UiNode => Space({
    key,
    width,
    height,
    cursor: "ns-resize",
    drag,
    onPress,
    // A click on its value types it; a drag from there still turns it.
    onClick: dial ? undefined : ({ y }) => {
      if (y >= valueY) type(true);
    },
    children: [
      Arc({ x: centreX, y: centreY, radius, from: -SWEEP / 2, to: SWEEP / 2, color: "fill-strong", width: 2.5 }),
      Arc({ x: centreX, y: centreY, radius, from: props.bipolar ? 0 : -SWEEP / 2, to: turn, color: "text", width: 2.5 }),
      Circle({ x: centreX, y: centreY, radius: cap, color: "fill-strong" }),
      Line({ from: along(cap / 3), to: along(cap * 0.76), color: "text", width: 2 }),
      // In the top right corner of the dial, clear of its arc.
      ...heldMark(held, centreX + radius - 2, centreY - radius + 2),
      ...texts(),
    ],
  });
  // A dial says no value, so none is typed on it.
  if (dial) return knob();
  const { get, set } = props.field === undefined ? valueOf(props) : numberAt(props.field, turns.label);
  const [low, high] = [Math.min(travel.min, travel.max), Math.max(travel.min, travel.max)];
  const taken = (number: number) => Math.min(Math.max(field?.integer ? Math.round(number) : number, low), high);
  // The field stands where the dial is, over the label it keeps.
  const typed = () => {
    const before = untrack(get);
    const done = (number: number | null) => {
      type(false);
      if (number !== null && taken(number) !== before) set(taken(number));
    };
    return h("div", { style: { width, height, align: "center", gap: 2 } },
      h("div", { style: { height: CELL_CONTROL, justify: "center" } }, typedField(key, before, unit, width - 8, done)),
      said ? Text({ variant: "label", children: label }) : null);
  };
  const node = element("div", { key: `${key}-keys`, layout: "cell", keys: ["enter"], onKey: () => type(true), style: { keep: true } }, [Show({ when: typing, children: typed, fallback: knob })]);
  return said ? ownLines(node, props.field === undefined ? `the Knob ${JSON.stringify(label)}` : `<Knob field=${JSON.stringify(props.field)} />`) : node;
}

/** How a number spreads over the travel of a knob, a fader or a lane, as `numbers` and a lane's `scale` say. */
export interface Travel {
  min: number;
  max: number;
  scale: "linear" | "log" | "fader";
}

/** The fader's scale, as the window's: 0 dB at 0.8 of the travel and +6 dB at the top, a power law on the amplitude. */
const FADER = { unity: 0.8, decade: 61.94, top: 6 } as const;

/**
 * Where `value` sits along `travel`: 0 at `min` and 1 at `max`, past them outside. A `"fader"`
 * travel is a volume in dB whatever its ends: -inf at 0, 0 dB at 0.8 and +6 dB at 1.
 */
export function positionOf({ min, max, scale }: Travel, value: number): number {
  if (scale === "log") return Math.log(value / min) / Math.log(max / min);
  if (scale === "fader") return Math.min(FADER.unity * 10 ** (value / FADER.decade), 1);
  return (value - min) / (max - min);
}

/**
 * The value at `position` along `travel`, as `positionOf` puts it: a fader gives -Infinity at 0.
 * Of a live position, a live value, such as the readout of a point as a drag moves it; a fader's
 * gives -120 dB and less near 0.
 */
export function valueAt(travel: Travel, position: number): number;
export function valueAt(travel: Travel, position: Live): Live;
export function valueAt({ min, max, scale }: Travel, position: Num): Num {
  if (typeof position !== "number") {
    if (scale === "log") return position.times(0).plus(max / min).pow(position).times(min);
    if (scale === "fader") return position.over(FADER.unity).decibels().times(FADER.decade / 20).min(FADER.top);
    return position.times(max - min).plus(min);
  }
  if (scale === "log") return min * (max / min) ** position;
  if (scale === "fader") return position <= 0 ? -Infinity : Math.min(FADER.decade * Math.log10(position / FADER.unity), FADER.top);
  return min + position * (max - min);
}

/** Where a level, an amplitude, sits along a fader. */
const faderLevel = (amplitude: Live) => amplitude.pow(20 / FADER.decade).times(FADER.unity).min(1);

/** What a meter shows: the peaks a child keeps as `now.peak` names them, channels of the audio input, or what the device plays. */
export type Peaks = { of: string } | { input: readonly number[] } | "device";

export type FaderProps = Either<{ field: string }, Kept<number>> & { key?: string; length: number; horizontal?: boolean; default?: number; /** The undo step of a field. */ label?: string; min?: number; max?: number; unit?: Unit; meter?: Peaks };

/** The level of `channel` of `peaks`; one channel of the input shows on both bars. */
const levelOf = (peaks: Peaks, channel: 0 | 1): Live =>
  peaks === "device" ? now.devicePeak(channel)
  : "of" in peaks ? now.peak(peaks.of, channel)
  : now.input(peaks.input[channel] ?? peaks.input[0] ?? 0);

/** Places a box `size` long from `from` along a travel, and `thickness` across from `side`. */
type Box = (from: Num, size: Num, side: number, thickness: number) => { x: Num; y: Num; width: Num; height: Num };

/** Boxes along a travel of `length`: up from its foot, or `horizontal` to the right. */
const along = (horizontal: boolean | undefined, length: number): Box => (from, size, side, thickness) => {
  if (horizontal) return { x: from, y: side, width: size, height: thickness };
  const end = typeof from === "number" && typeof size === "number" ? from + size : typeof from === "number" ? (size as Live).plus(from) : from.plus(size);
  return { x: side, y: typeof end === "number" ? length - end : end.negate().plus(length), width: thickness, height: size };
};

/** The two bars of `peaks` along `length`, `thickness` thick and `gap` apart from `side`: green up to -6 dB and yellow above on the fader's scale, with a peak line that holds. */
function levelBars(peaks: Peaks, length: number, box: Box, side: number, thickness: number, gap: number): UiNode[] {
  const hot = positionOf({ min: 0, max: 0, scale: "fader" }, -6) * length;
  return ([0, 1] as const).flatMap((channel) => {
    const level = levelOf(peaks, channel);
    const bar = faderLevel(level).fall(0.5).times(length);
    const peak = faderLevel(level).hold(1.5).fall(0.5).times(length);
    const at = side + channel * (thickness + gap);
    return [
      Rect({ ...box(0, length, at, thickness), color: "fill" }),
      Rect({ ...box(0, bar.min(hot), at, thickness), color: "green" }),
      Rect({ ...box(hot, bar.minus(hot).max(0), at, thickness), color: "yellow" }),
      Rect({ ...box(peak.minus(1), 1, at, thickness), color: "text", opacity: peak.gt(0.01 * length) }),
    ];
  });
}

/**
 * A fader, as the app's volumes look: a handle a drag moves along `length` points, up or
 * `horizontal`, over the level bars of `meter` when given. What it moves is as a `Knob`'s. A field
 * keeps its own range and scale unless `min` and `max` say; what code keeps, without them, moves
 * on the fader's scale in dB. A double click puts it at `default`. It says no number: put a
 * `NumberBox` or a `Readout` of its field beside it.
 */
export function Fader(props: FaderProps): UiNode {
  const { length, horizontal, meter } = props;
  const { travel, turns } = numberOf("Fader", { ...props, label: undefined });
  const scale = travel ?? (props.field === undefined ? FADER_TRAVEL : undefined);
  const moves = { ...turns, ...(scale?.scale === "fader" ? { fader: true } : {}), travel: length, label: props.label ?? turns.label };
  const { key, fraction, held, drag, onPress } = turned("fader", props, scale, moves, horizontal ? "x" : "y");
  const bars = meter === undefined ? [] : levelBars(meter, length, along(horizontal, length), 8, 5, 2);
  const at = () => read(fraction).times(length).minus(3);
  const handle = horizontal
    ? Rect({ x: at, y: 0, width: 6, height: 28, radius: 3, color: "text" })
    : Rect({ x: 0, y: () => at().negate().plus(length - 6), width: 28, height: 6, radius: 3, color: "text" });
  return Space({
    key,
    width: horizontal ? length : 28,
    height: horizontal ? 28 : length,
    cursor: horizontal ? "ew-resize" : "ns-resize",
    drag,
    onPress,
    children: [...bars, handle, ...heldMark(held, horizontal ? length - 2 : 26, 2)],
  });
}

/** The units of a `Label` that show a tick of the clock, not a number. */
const TICK_UNITS = ["position", "time", "signature", "beats", "note"] as const;
type TickUnit = (typeof TICK_UNITS)[number];

export type NumberBoxProps = Either<
  { field: string },
  /** `show` is what it shows in place of what code keeps, as a `Label` does: `now.bpm` of a tempo that changes as the piece plays, or `now.tick` in `"beats"`. */
  Kept<number> & { show?: Num }
> & Quiet & {
  /** Names its space. */
  key: string;
  /** The field's own when left out. */
  min?: number;
  max?: number;
  /** It moves in whole steps of this size, and a typed value rounds to it: 1 for what code keeps and for a whole number field when left out; any other field turns over its range, as a knob. */
  step?: number;
  /** The only values it takes, in order, such as note values: a drag steps through them, and a typed value goes to the nearest. */
  values?: readonly number[];
  /** Points of a drag for one step: 10 when left out. */
  per?: number;
  /** How it shows the number, as a `Label` does: the field's unit when left out. */
  unit?: Unit | TickUnit;
  /** The undo step of a field: "Change <its label>" when left out. */
  label?: string;
  width: number;
  /** `md` when left out. A small one says its number as a `label`, the others as `body` text. */
  size?: ControlSize;
  /** Its height in points where no size fits, such as a readout under a fader. */
  height?: number;
  align?: "left" | "center" | "right";
};

/** The heights of `ControlSize`, for the controls drawn here. */
const CONTROL_HEIGHT = { sm: 24, md: 28, lg: 32 } as const;

/** The semitones of each note letter above C. */
const SEMITONES = { c: 0, d: 2, e: 4, f: 5, g: 7, a: 9, b: 11 } as const;

/** The notes of an octave, as a `"pitch"` reads them. */
const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"] as const;

/** `value` with three decimals at most, as a field for typing shows it. */
const short = (value: number) => String(Number(value.toFixed(3)));

/**
 * The number that `text` says as `unit` reads one, or `null`: `50` and `50%` are 0.5 of a
 * percent, `2k` is 2000 Hz, `1.5 s` is 1500 ms, `F#3` is a pitch, `25L` a pan, and `-6 dB`
 * and `-inf` what they say.
 */
function typedNumber(text: string, unit: NumberBoxProps["unit"]): number | null {
  const plain = text.trim().replace("−", "-").replace(/\s/g, "").toLowerCase();
  const note = unit === "pitch" ? /^([a-g])([#b]?)(-?\d+)$/.exec(plain) : null;
  if (note) {
    const [, letter = "c", accidental, octave] = note;
    return SEMITONES[letter as keyof typeof SEMITONES] + (accidental === "#" ? 1 : accidental === "b" ? -1 : 0) + (Number(octave) + 1) * 12;
  }
  if (plain.startsWith("-inf")) return -Infinity;
  if (unit === "pan" && plain === "c") return 0;
  const value = Number.parseFloat(plain);
  if (!Number.isFinite(value)) return null;
  if (unit === "percent") return value / 100;
  if (unit === "pan") return (plain.endsWith("l") ? -Math.abs(value) : value) / 100;
  if (unit === "hz" && /\dk/.test(plain)) return value * 1000;
  if (unit === "ms" && /\ds$/.test(plain)) return value * 1000;
  if (unit === "seconds" && plain.endsWith("ms")) return value / 1000;
  return value;
}

/** What the field for typing `value` opens on: the number as `unit` reads it, which `typedNumber` reads back. */
function typedText(value: number, unit: NumberBoxProps["unit"]): string {
  if (!Number.isFinite(value)) return "-inf";
  if (unit === "percent") return short(value * 100);
  if (unit === "pan") return value === 0 ? "C" : `${short(Math.abs(value) * 100)}${value < 0 ? "L" : "R"}`;
  if (unit === "pitch" && Number.isInteger(value) && value >= 0) return `${NOTE_NAMES[value % 12]}${Math.floor(value / 12) - 1}`;
  return short(value);
}

/** The field a number control shows in its place while its number is typed: `done` gets the number typed, or `null` for none. */
const typedField = (key: string, value: number, unit: NumberBoxProps["unit"], width: number, done: (typed: number | null) => void, size: ControlSize = "sm"): UiNode =>
  h("div", { key: `${key}-typed` }, Input({ value: typedText(value, unit), width, size, focus: true, onCommit: (text) => done(typedNumber(text, unit)), onCancel: () => done(null) }));

/**
 * A number to drag up and down, or double-click and type, as the transport's tempo is: a field,
 * or what code keeps, such as the tempo map, see `ValueProps`. Either changes as it is dragged:
 * a field's drag and a typed value are one undo step each, and what code keeps hears each value.
 * Tab reaches it: enter types, and the arrows step it.
 */
export function NumberBox(props: NumberBoxProps): h.JSX.Element {
  const { key, width, values, field, size = "md" } = props;
  const told = numberOf("NumberBox", { ...props, unit: undefined, label: undefined });
  const unit = props.unit ?? told.unit;
  const [min, max] = [props.min ?? told.field?.min, props.max ?? told.field?.max];
  const step = props.step ?? (field === undefined || told.field?.integer ? 1 : undefined);
  const height = props.height ?? CONTROL_HEIGHT[size];
  // A small one says its number as a label, the others as body text: lines of 16 and 18 points.
  const [variant, line] = size === "sm" ? (["label", 16] as const) : (["body", 18] as const);
  const label = props.label ?? told.turns.label;
  const { get, set } = field === undefined ? valueOf(props) : numberAt(field, label);
  const [typing, type] = signal(false);
  const clamped = (number: number) => Math.min(Math.max(number, min ?? -Infinity), max ?? Infinity);
  /** A typed or turned value as it takes it: one of `values`, or on the grid of `step`. */
  const taken = (number: number) =>
    values ? values.reduce((best, each) => (Math.abs(each - number) < Math.abs(best - number) ? each : best), values[0] ?? number)
    : clamped(step === undefined ? number : Math.round(number / step) * step);
  const give = (number: number) => {
    if (number !== untrack(get)) set(number);
  };
  const x = props.align === "center" ? width / 2 : props.align === "right" ? width : 0;
  const text = { x, y: Math.round((height - line) / 2), variant, color: "text", align: props.align } as const;
  const shows = field !== undefined ? now.value(field) : props.show !== undefined ? Live.of(props.show) : () => Live.of(finite(get()));
  const valueUnit = unit === undefined || TICK_UNITS.includes(unit as TickUnit) ? undefined : (unit as Unit);
  const stepped = step === undefined ? {} : { step, per: props.per };
  /** What its drag turns. What code keeps moves in steps: through `values`, or of `step`. */
  const kept = field !== undefined ? undefined
    : values ? keptTurns(get, give, (from, moved) => values[Math.min(Math.max(Math.max(values.indexOf(taken(from)), 0) + moved, 0), values.length - 1)] ?? from)
    : keptTurns(get, give, (from, moved) => taken(from + moved * (step ?? 1)));
  const turns: DragTurns = kept
    ? { onTurn: kept.onTurn, step: 1, per: props.per }
    : { value: field ?? "", min, max, unit: valueUnit, fader: told.turns.fader, label, ...stepped };
  const box = (): UiNode =>
    Space({
      key, width, height, cursor: "ns-resize",
      onPress: ({ clicks, button }: Press) => {
        kept?.onPress();
        if (button === "left" && clicks === 2) type(true);
      },
      drag: { y: turns },
      children: [Label({ ...text, text: shows, unit, opacity: quietly(props.quiet, { field, key }) })],
    });
  // Enter types a number. The arrows are the window's, as a knob's.
  return asCell(h("div", { key: `${key}-keys`, keys: ["enter"], onKey: () => type(true), style: { radius: "sm", keep: true } }, Show({
    when: typing,
    children: () => {
      const before = untrack(get);
      return typedField(key, before, unit, width, (number) => {
        type(false);
        if (number !== null && taken(number) !== before) set(taken(number));
      }, size);
    },
    fallback: box,
  })), told.label);
}

export type ReadoutProps = Quiet & {
  field: string;
  /** The field's own when left out. */
  unit?: Unit;
  width: number;
  align?: "left" | "center" | "right";
};

/**
 * What a field plays, in words, as a line of `label` text: the number of a fader or of a dial
 * that shows none, beside it. With `quiet` it shows while that control is touched, see `Quiet`.
 */
export function Readout({ field, width, align, quiet, ...props }: ReadoutProps): UiNode {
  const { unit } = numberOf("Readout", { ...props, field });
  const value = now.value(field);
  const x = align === "center" ? width / 2 : align === "right" ? width : 0;
  return Space({ width, height: 16, children: [Label({ x, y: 0, text: value, unit, color: "text", align, opacity: quietly(quiet, { field, key: field }) })] });
}

/** A number field as a `NumberBox` reads and sets it: -inf as `"-inf"`, as a fader holds it. */
function numberAt(path: string, label?: string) {
  const field = valueOf<unknown>({ field: path });
  return {
    get: () => {
      const found = field.get();
      return found === "-inf" ? -Infinity : typeof found === "number" ? found : Number.NaN;
    },
    set: (number: number) => field.set(number === -Infinity ? "-inf" : Number(short(number)), label),
  };
}

/** The record of the child at `path` below `top`, or `top` itself. */
function recordAt(top: { state: unknown; children: unknown }, path: string | undefined): { state: Record<string, unknown>; tool?: string } | undefined {
  let at = top as { state: Record<string, unknown>; tool?: string; children: readonly { name: string; tool: string; state: Record<string, unknown>; children: unknown }[] };
  for (const name of path?.split("/") ?? []) {
    const next = at.children.find((child) => child.name === name);
    if (!next) return undefined;
    at = next as typeof at;
  }
  return at;
}

/** An option of a `Select`: its value, and what its list says of it. */
export interface SelectOption<T> {
  value: T;
  label: string;
  detail?: string;
  icon?: IconName;
  disabled?: boolean;
}

/** What a control of options is on: a `choice` field, whose options it then takes unless `options` says, or what code keeps with its `options`. */
type Chosen<T, Option> = Either<{ field: string; options?: Bind<readonly Option[]> }, Kept<T> & { options: Bind<readonly Option[]> }>;

/** The options of a control: what its props say, else those of the choice field it is on, each named in words. */
function optionsOf<T, Option extends { value: T; label: string }>(kind: string, props: { field?: string; options?: Bind<readonly Option[]> }): () => readonly Option[] {
  const { field, options } = props;
  if (options !== undefined) return () => read(options);
  const found = field === undefined ? undefined : fieldOf(field);
  if (found?.kind !== "choice") throw new Error(`${kind}: ${JSON.stringify(field)} is no choice field. Name a choice field, or give it options.`);
  const told = found.options.map((value) => ({ value, label: words(String(value)) })) as unknown as Option[];
  return () => told;
}

/** `onChange` hears a pick of another option. */
export type SelectProps<T> = Chosen<T, SelectOption<T>> & {
  /** Names it for tests: `div-<key>`. */
  key?: string;
  /** The heading over its list. */
  title?: string;
  /** What it says while `value` is none of the options: the value when left out. */
  placeholder?: string;
  /** Of its list, in points: 160 when left out. */
  menuWidth?: number;
  /** `md` when left out. A small one says it as a `label`, the others as a `heading`. */
  size?: ControlSize;
  /** A field above its list that keeps the options whose label holds what is typed. */
  search?: boolean;
};

/** Shows the option `value` is and opens the list of `options` under it, as the window's selects. */
export function Select<const T extends string | number | boolean | null>(props: SelectProps<T>): UiNode {
  const { openMenu } = drawing();
  const { get, set } = valueOf(props);
  const options = optionsOf("Select", props);
  const size = props.size ?? "md";
  const said = () => {
    const value = get();
    return options().find((option) => option.value === value)?.label ?? props.placeholder ?? String(value);
  };
  const open = async () => {
    const [value, listed] = [get(), options()];
    const items = listed.map(({ label, detail, icon, disabled, value: each }) => ({ label, detail, icon, disabled, checked: each === value, group: props.title }));
    const picked = await openMenu(items, { width: props.menuWidth ?? 160, search: props.search });
    const option = picked === null ? undefined : listed[picked];
    if (option && option.value !== value) set(option.value);
  };
  const style = { direction: "row", align: "center", gap: "xs", height: CONTROL_HEIGHT[size], paddingX: "sm", radius: "md", background: "fill", hover: { background: "fill-strong" } } as const;
  // Medium, as what the window's own controls say.
  const text = size === "sm" ? Text({ variant: "label", tone: "text", style: { weight: "medium" }, children: said }) : Text({ variant: "heading", children: said });
  const node = h("div", { key: props.key, style, tabStop: true, onClick: open }, text, Icon({ name: "chevron-down", size: 12, color: "text-muted" }));
  return asCell(node, cellLabel(props));
}

export type SegmentedControlProps<T> = Chosen<T, Pick<SelectOption<T>, "value" | "label">> & {
  /** Names it for tests: `segmented-<key>`, and each option `segment-<place>`. */
  key?: string;
  /** `md` when left out. */
  size?: ControlSize;
  disabled?: Bind<boolean>;
};

/** One of a few options side by side, as the window's pages of a card. */
export function SegmentedControl<const T extends string | number | boolean | null>(props: SegmentedControlProps<T>): UiNode {
  const { get, set } = valueOf(props);
  const options = optionsOf("SegmentedControl", props);
  const node = element("segmented", {
    key: props.key,
    size: props.size,
    options: () => options().map((option) => option.label),
    chosen: () => {
      const value = get();
      return options().findIndex((option) => option.value === value);
    },
    disabled: props.disabled,
    onChange: (place: number) => {
      const option = untrack(options)[place];
      if (option && option.value !== untrack(get)) set(option.value);
    },
  }, []);
  return asCell(node, cellLabel(props));
}

export type SliderProps = { field: string; key?: string; min?: number; max?: number; default?: number; unit?: Unit; label?: string; bipolar?: boolean; width?: number };

/**
 * A field or a number input on a short line, as the window's sliders: a drag sideways, ten times
 * finer with shift, a double click to `default`, the arrows step. Over the field's range unless
 * `min` and `max` say, on a log scale for `"hz"`; `bipolar` draws from 0. 120 points wide unless
 * `width` says.
 */
export function Slider({ field, key, bipolar, width, ...props }: SliderProps): UiNode {
  const { label, travel, turns, unranged } = numberOf("Slider", { ...props, field });
  if (!travel || travel.scale === "fader") throw unranged();
  const { min, max } = travel;
  const node = element("slider", { key, bipolar, width, field, min, max, unit: turns.unit, default: turns.default, label: turns.label, value: now.value(field) }, []);
  return asCell(node, label);
}

/**
 * A quiet line for what the composer should know, as the window's: `tone` is `error` when left
 * out, `progress` runs from 0 to 1.
 */
export const Notice = ({ action, onDismiss, ...props }: { key?: string; message: Bind<string>; tone?: Bind<"error" | "warning" | "info">; progress?: Bind<number>; action?: { label: Bind<string>; onClick: () => void }; onDismiss?: () => void }): UiNode =>
  element("notice", { ...props, action: action?.label, onClick: action?.onClick, onClose: onDismiss }, []);

export type StepsProps = ValueProps<readonly number[]> & {
  key?: string;
  /** What a step turned on holds, unless it held another before: the default of the field's items when that is on, else their max, or 1. */
  max?: number;
  playing?: string;
  /** Its steps run on their own, not with the piece: the ring of `playing` shows while the piece is stopped too. */
  ownClock?: boolean;
  /** What the row is, at its left: the field's label, else its name in words, when left out; `""` for none. Rows with a label, one over the other, line up. */
  label?: string;
  /** A row that changes another row, such as accents or slides: it reads quieter than a row of notes. */
  secondary?: boolean;
};

/**
 * A row of steps on a list of numbers, such as a `pattern` field, one per number, in groups of
 * four. A step is lit as far up as it is strong, from the items' min, off, to their max (0 and 1
 * for what code keeps), so a soft hit shows as one. A click turns a step off, or on again as
 * strong as it was. Tab reaches it: the arrows move along it, and space or enter turns a step.
 * `playing` names an output, as `now.value` does, whose value is the step that plays, which a
 * green ring shows while the piece plays.
 */
export function Steps(props: StepsProps): UiNode {
  const field = props.field === undefined ? undefined : fieldOf(props.field);
  const item = field?.kind === "list" && field.item.kind === "number" ? field.item : undefined;
  const [off, full] = [item?.min ?? 0, item?.max ?? 1];
  const on = props.max ?? (item?.default !== undefined && item.default > off ? item.default : full);
  const { get, set } = valueOf(props);
  const list = () => get() ?? [];
  const { clock } = drawing();
  const step = props.playing === undefined ? undefined : now.value(props.playing);
  /** What each step held before it was turned off. Not saved: a view made again turns a step on at `on`. */
  const held = new Map<number, number>();
  return element("steps", {
    key: props.key ?? (props.field !== undefined ? `steps-${props.field}` : undefined),
    label: props.label ?? (props.field === undefined ? undefined : labelOf(props.field, field)),
    secondary: props.secondary,
    levels: () => list().map((value) => within((value - off) / (full - off))),
    // At rest a ring on the first step would read as a selection.
    playing: step && (props.ownClock ? step : () => (clock.playing ? step : undefined)),
    onChange: (step: number) => {
      const changed = [...untrack(list)];
      const was = changed[step] ?? off;
      if (was > off) held.set(step, was);
      changed[step] = was > off ? off : (held.get(step) ?? on);
      set(changed, was > off ? "Turn a step off" : "Turn a step on");
    },
  }, []);
}

export type MeterProps =
  | { value: string | Bind<Num>; label?: string; peaks?: never }
  | { peaks: Peaks; horizontal?: boolean; length?: number; key?: string; value?: never };

/**
 * A bar that shows a number from 0 to 1, `value`: one named as `now.value` names it, such as a
 * level an output sends, or any live value. Or a stereo level, `peaks`, as the window's own meters
 * look: two bars on the fader's scale with a peak line that holds, and a red light for 3 seconds
 * after it clips. Up the side unless `horizontal`; `length` along its bars, light included.
 */
export function Meter(props: MeterProps): UiNode {
  if (props.peaks !== undefined) return levelMeter(props.peaks, props.horizontal, props.length, props.key);
  const level = () => {
    const value = read(props.value);
    return (typeof value === "string" ? now.value(value) : Live.of(value)).clamp(0, 1).times(64);
  };
  const bar = Space({
    width: 8,
    height: 64,
    children: [
      Rect({ x: 0, y: 0, width: 8, height: 64, radius: 2, color: "fill" }),
      Rect({ x: 0, y: () => level().negate().plus(64), width: 8, height: level, radius: 2, color: "green" }),
    ],
  });
  return h("div", { style: { align: "center", gap: "xs" } }, bar, props.label && Text({ variant: "label", children: props.label }));
}

/** The bars of `peaks` with the light at their loud end, as `Meter` draws them. */
function levelMeter(peaks: Peaks, horizontal: boolean | undefined, length = horizontal ? 45 : 118, key?: string): UiNode {
  const [thickness, gap, light] = [horizontal ? 3 : 5, 2, 3];
  const across = thickness * 2 + gap;
  const clipped = levelOf(peaks, 0).max(levelOf(peaks, 1)).gt(1).hold(3);
  const look = { radius: 1, color: "red", opacity: clipped } as const;
  return Space({
    key,
    width: horizontal ? length : across,
    height: horizontal ? across : length,
    children: [
      ...levelBars(peaks, length - light - gap, along(horizontal, length), 0, thickness, gap),
      horizontal ? Rect({ ...look, x: length - light, y: 0, width: light, height: across }) : Rect({ ...look, x: 0, y: 0, width: across, height: light }),
    ],
  });
}

/** Fields or number inputs named as `now.value` names them, or what code keeps, from 0 to 1 across and up. */
export type PadProps = { key?: string; size?: number } & Either<{ field: { x: string; y: string } }, Kept<{ x: number; y: number }>>;

/** A square to play with the pointer: across moves `x` and up moves `y`. */
export function Pad(props: PadProps): UiNode {
  const size = props.size ?? 120;
  const look = { width: size, height: size, background: "fill", radius: "md", cursor: "crosshair" } as const;
  const dot = (x: Bind<Num>, y: Bind<Num>) => Circle({ x, y, radius: 5, color: "text" });
  if (props.field !== undefined) {
    const { x, y } = props.field;
    return Space({
      ...look,
      key: props.key ?? `pad-${x}-${y}`,
      drag: { x: { value: x, absolute: true }, y: { value: y, absolute: true } },
      children: [dot(now.value(x, { fraction: true }).times(size), now.value(y, { fraction: true }).negate().plus(1).times(size))],
    });
  }
  const { value, onChange } = props;
  // Each axis of the drag tells its own place, and code hears the two together; escape puts both back.
  let [pressed, at] = [untrack(() => read(value)), untrack(() => read(value))];
  const turns = (axis: "x" | "y"): DragTurns => ({
    onTurn: (turn) => {
      at = { ...at, [axis]: "at" in turn ? turn.at : pressed[axis] };
      onChange(at);
    },
    min: 0,
    max: 1,
    absolute: true,
  });
  return Space({
    ...look,
    key: props.key ?? controlId("pad"),
    onPress: () => {
      pressed = at = untrack(() => read(value));
    },
    drag: { x: turns("x"), y: turns("y") },
    children: [dot(() => read(value).x * size, () => (1 - read(value).y) * size)],
  });
}

/** `tone_hz` is `Tone hz`. */
export function words(name: string): string {
  const spaced = name.replaceAll("_", " ");
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}
