// Components made of the blocks above.

import { Arc, Button, Circle, Color, h, Icon, IconName, Input, Label, Line, Rect, Space, Style, Toggle } from "./drawing";
import { Fields, Inputs, NumberField, Unit } from "./fields";
import { Drawing } from "./host";
import { Drag, DragResult, DragTurns, Live, now, Num, Press } from "./live";
import { Child, currentCard, element, For, Show, UiNode } from "./nodes";
import { Accessor, Bind, signal, untrack } from "./reactivity";
import { Context } from "./tools";

/** The view that draws now, for the components below, which read its record and fields. */
function drawing(): Drawing {
  return currentCard().drawn;
}

/** A value code keeps, such as in `memory`: a control shows it and `onChange` hears a drag once, at its end. */
export type Kept<T> = { value: Bind<T>; onChange: (value: T) => void };

/** `A` or `B`, never a mix: each leaves out the keys of the other. */
type Either<A, B> = (A & { [K in Exclude<keyof B, keyof A>]?: never }) | (B & { [K in Exclude<keyof A, keyof B>]?: never });

/**
 * What a control shows and changes: a `field` as `now.value` names it, such as `"cutoff"` or
 * `"bass/mix.gain_db"`, whose change is one undo step; or a value code keeps, see `Kept`.
 */
export type ValueProps<T> = Either<{ field: string }, Kept<T>>;

const read = <V,>(bound: Bind<V>): V => (typeof bound === "function" ? (bound as Accessor<V>)() : bound);

/** A number as a live value, to combine with others. */
const liveOf = (value: Num): Live => (typeof value === "number" ? new Live({ op: "plus", a: value, b: 0 }) : value);

const within = (position: number) => Math.min(Math.max(position, 0), 1);

let controls = 0;
/** The drag id and key of a control on what code keeps, when its props name none. */
const controlId = (kind: string) => `${kind}-${++controls}`;

/** A field, or what code keeps, as a control reads and sets it. A field's change is one undo step of `label`. */
function valueOf<T>(props: ValueProps<T>): { get: () => T; set: (value: T, label?: string) => void } {
  if (props.field === undefined) {
    const { value, onChange } = props;
    return { get: () => read(value), set: (value) => onChange(value) };
  }
  const { state, children, update } = drawing();
  const { child, name } = splitPath(props.field);
  return {
    get: () => recordAt({ state, children }, child)?.state[name] as T,
    set: (value, label) =>
      update(label ?? `Change ${name.replaceAll("_", " ")}`, (part) => {
        const record = recordAt(part, child);
        if (record) record.state[name] = value;
      }),
  };
}

/** The arc of a knob runs from -135 to +135 degrees, in turns. */
const SWEEP = 0.75;

/** Points of a drag for the whole range of a knob, as the window's. */
const TRAVEL = 200;

/** How a knob spreads a number from `min` to `max`, as the window's knobs: on a log scale for `"hz"`. */
const knobTravel = (min: number, max: number, unit?: Unit | "pan"): Travel => ({ min, max, scale: unit === "hz" && min > 0 ? "log" : "linear" });

/** The value at `position` along a knob's `travel`, as `valueAt` puts it, worked out in the window. */
function liveValueAt({ min, max, scale }: Travel, position: Live): Live {
  if (scale === "log") return new Live({ op: "pow", a: max / min, b: position }).times(min);
  return position.times(max - min).plus(min);
}

/**
 * What a knob or a fader shows and the drag that turns it, `along` an axis: a field the window
 * turns as the drag goes; or what code keeps, which shows where the drag went from it until
 * `onChange`, at its end, gives it a new value.
 */
function turned(kind: string, props: ValueProps<number> & { key?: string; played?: boolean }, travel: Travel, turns: Omit<DragTurns, "value">, along: "x" | "y") {
  if (props.field !== undefined) {
    const { field: name, played } = props;
    return {
      key: props.key ?? `${kind}-${name}`,
      value: () => now.value(name, { played }),
      fraction: () => now.value(name, { played, fraction: true }),
      drag: { [along]: { value: name, ...turns } } as Drag,
      onPress: undefined,
    };
  }
  const { value, onChange } = props;
  const key = props.key ?? controlId(kind);
  const length = turns.travel ?? TRAVEL;
  const { dx, dy } = now.drag(key);
  const moved = along === "x" ? dx.over(length) : dy.over(-length);
  const position = () => positionOf(travel, read(value));
  const fraction = () => moved.plus(position()).clamp(0, 1);
  const onEnd = (drag: DragResult) => onChange(valueAt(travel, within(untrack(position) + (along === "x" ? drag.dx : -drag.dy) / length)));
  const onPress = ({ clicks }: Press) => {
    if (clicks === 2 && turns.default !== undefined) onChange(turns.default);
  };
  return { key, value: () => liveValueAt(travel, fraction()), fraction, drag: { id: key, axis: along, onEnd }, onPress };
}

export type KnobProps = ValueProps<number> & { key?: string; label?: string; min: number; max: number; default: number; unit?: Unit | "pan"; played?: boolean; bipolar?: boolean; size?: "sm" | "md" };

/**
 * A knob on a number, as the app's knobs look: a field of the record such as `rate`, whose drag
 * is smooth and one undo step, a number input, which nothing saves, or what code keeps, see
 * `ValueProps`. With `played`, it shows the field as it plays, so it moves with a lane. A
 * double click puts it at `default`. A `bipolar` one, such as a pan, draws its arc from the top;
 * `unit: "pan"` reads `C`, `25L`, `100R`. It shows `label` and the value under it; `size: "sm"`
 * is a 28-point dial alone.
 */
export function Knob(props: KnobProps): UiNode {
  const { label, min, max, unit, bipolar } = props;
  const small = props.size === "sm";
  const turns = { min, max, unit: unit === "pan" ? undefined : unit, default: props.default, label: label && `Change ${label.toLowerCase()}` };
  const { key, value, fraction, drag, onPress } = turned("knob", props, knobTravel(min, max, unit), turns, "y");
  const angle = () => fraction().times(SWEEP).minus(SWEEP / 2);
  const [centreX, centreY, radius, cap] = small ? [14, 14, 11, 7] : [28, 18, 16.75, 10.5];
  const along = (distance: number) => () => {
    const radians = angle().times(2 * Math.PI);
    return [radians.sin().times(distance).plus(centreX), radians.cos().times(-distance).plus(centreY)] as const;
  };
  const texts = small ? [] : [
    ...(label === undefined ? [] : [Label({ x: centreX, y: 38, text: label, color: "gray-800", size: 12, align: "center" })]),
    Label({ x: centreX, y: label === undefined ? 38 : 54, text: value, unit, color: "gray-950", size: 12, align: "center" }),
  ];
  return Space({
    key,
    width: small ? 28 : 56,
    height: small ? 28 : label === undefined ? 56 : 72,
    cursor: "ns-resize",
    drag,
    onPress,
    children: [
      Arc({ x: centreX, y: centreY, radius, from: -SWEEP / 2, to: SWEEP / 2, color: "alpha/10", width: 2.5 }),
      Arc({ x: centreX, y: centreY, radius, from: bipolar ? 0 : -SWEEP / 2, to: angle, color: "gray-950", width: 2.5 }),
      Circle({ x: centreX, y: centreY, radius: cap, color: "gray-300" }),
      Line({ from: along(cap / 3), to: along(cap * 0.76), color: "gray-950", width: 2 }),
      ...texts,
    ],
  });
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

export type FaderProps = ValueProps<number> & { key?: string; length: number; horizontal?: boolean; default?: number; label?: string; min?: number; max?: number; unit?: Unit; played?: boolean; meter?: Peaks };

/**
 * A fader on a number, as the app's volumes look: a handle a drag moves along `length` points,
 * up or, `horizontal`, to the right, over two bars of `meter` when given, green up to -6 dB and
 * yellow above with a peak line that holds, on the fader's scale. What it moves is as a `Knob`'s; it
 * takes the number's range and scale unless `min` and `max` say, so a `gain_db` of a `fader`
 * moves on the fader's scale, as does what code keeps without them. A double click puts it at
 * `default`.
 */
export function Fader(props: FaderProps): UiNode {
  const { length, horizontal, meter, min, max, unit } = props;
  const travel: Travel = min !== undefined && max !== undefined ? knobTravel(min, max, unit) : { min: 0, max: 0, scale: "fader" };
  const turns = { min, max, unit, travel: length, default: props.default, label: props.label };
  const { key, fraction, drag, onPress } = turned("fader", props, travel, turns, horizontal ? "x" : "y");
  /** A box `size` long from `from` along the travel, from its foot, and `thickness` across from `side`. */
  const box = (from: Num, size: Num, side: number, thickness: number) => {
    if (horizontal) return { x: from, y: side, width: size, height: thickness };
    const end = typeof from === "number" && typeof size === "number" ? from + size : typeof from === "number" ? (size as Live).plus(from) : from.plus(size);
    return { x: side, y: typeof end === "number" ? length - end : end.negate().plus(length), width: thickness, height: size };
  };
  const hot = positionOf({ min: 0, max: 0, scale: "fader" }, -6) * length;
  const bars = meter === undefined ? [] : ([0, 1] as const).flatMap((channel) => {
    const level =
      meter === "device" ? now.devicePeak(channel)
      : "of" in meter ? now.peak(meter.of, channel)
      : now.input(meter.input[channel] ?? meter.input[0] ?? 0);
    const bar = faderLevel(level).fall(0.5).times(length);
    const peak = faderLevel(level).hold(1.5).fall(0.5).times(length);
    const side = 8 + channel * 7;
    return [
      Rect({ ...box(0, length, side, 5), color: "alpha/5" }),
      Rect({ ...box(0, bar.min(hot), side, 5), color: "green" }),
      Rect({ ...box(hot, bar.minus(hot).max(0), side, 5), color: "yellow" }),
      Rect({ ...box(peak.minus(1), 1, side, 5), color: "gray-950", opacity: peak.gt(0.01 * length) }),
    ];
  });
  const at = () => fraction().times(length).minus(3);
  const handle = horizontal
    ? Rect({ x: at, y: 0, width: 6, height: 28, radius: 3, color: "gray-950" })
    : Rect({ x: 0, y: () => at().negate().plus(length - 6), width: 28, height: 6, radius: 3, color: "gray-950" });
  return Space({
    key,
    width: horizontal ? length : 28,
    height: horizontal ? 28 : length,
    cursor: horizontal ? "ew-resize" : "ns-resize",
    drag,
    onPress,
    children: [...bars, handle],
  });
}

/** The units of a `Label` that show a tick of the clock, not a number. */
const TICK_UNITS = ["position", "time", "signature", "beats", "note"] as const;
type TickUnit = (typeof TICK_UNITS)[number];

export type NumberBoxProps = Either<
  { field: string },
  /** `show` is what it shows in place of what code keeps, as a `Label` does: `now.bpm` of a tempo that changes as the piece plays, or `now.tick` in `"beats"`. */
  Kept<number> & { show?: Num }
> & {
  /** Names its space and its drag, as `now.drag(key)`. */
  key: string;
  min?: number;
  max?: number;
  /** What a drag of what code keeps moves by, and what a typed value rounds to: 1 when left out. */
  step?: number;
  /** The only values it takes, in order, such as note values: a drag steps through them, and a typed value goes to the nearest. */
  values?: readonly number[];
  /** Points of a drag for one step of what code keeps: 10 when left out. */
  per?: number;
  /** How it shows the number, as a `Label` does. */
  unit?: Unit | "pan" | TickUnit;
  /** A field as it plays, see `now.value`. */
  played?: boolean;
  /** The undo step of a field: "Change <field>" when left out. */
  label?: string;
  width: number;
  /** 18 points when left out. */
  height?: number;
  /** Of its text, 14 points when left out. */
  size?: number;
  color?: Color;
  align?: "left" | "center" | "right";
};

/** A number that `text` says, or `null`: `-6 dB`, `2k` for 2000 in Hz, `-inf`. */
function typedNumber(text: string, unit: NumberBoxProps["unit"]): number | null {
  const plain = text.trim().replace("−", "-");
  if (/^-\s*inf/i.test(plain)) return -Infinity;
  const value = Number.parseFloat(plain);
  if (!Number.isFinite(value)) return null;
  return unit === "hz" && /\dk/i.test(plain.replace(/\s/g, "")) ? value * 1000 : value;
}

/**
 * A number to drag up and down, or double-click and type, as the transport's tempo is: a field,
 * or what code keeps, such as the tempo map, see `ValueProps`. A field's drag and a typed value
 * are one undo step each; what code keeps hears a value once, at the end of a drag.
 */
export function NumberBox(props: NumberBoxProps): h.JSX.Element {
  const { key, unit, width, values, field } = props;
  const height = props.height ?? 18;
  const per = props.per ?? 10;
  const step = props.step ?? 1;
  const { get, set } = field === undefined ? valueOf(props) : numberField(field, props.label);
  const [typing, type] = signal(false);
  const clamped = (number: number) => Math.min(Math.max(number, props.min ?? -Infinity), props.max ?? Infinity);
  /** A typed or dragged value as it takes it: one of `values`, or on the grid of `step`. */
  const taken = (number: number) =>
    values ? values.reduce((best, each) => (Math.abs(each - number) < Math.abs(best - number) ? each : best), values[0] ?? number)
    : field !== undefined ? clamped(number) : clamped(Math.round(number / step) * step);
  const x = props.align === "center" ? width / 2 : props.align === "right" ? width : 0;
  const text = { x, y: 0, size: props.size ?? 14, color: props.color ?? "gray-950", align: props.align } as const;
  const shows = props.field !== undefined ? now.value(props.field, { played: props.played }) : props.show !== undefined ? liveOf(props.show) : () => liveOf(get());
  const shown = { ...text, text: shows, unit };
  const valueUnit = unit === undefined || TICK_UNITS.includes(unit as TickUnit) ? undefined : (unit as Unit | "pan");
  const onPress = ({ clicks, button }: Press) => {
    if (button === "left" && clicks === 2) type(true);
  };
  const box = (): UiNode => {
    if (field !== undefined) {
      return Space({
        key, width, height, cursor: "ns-resize", onPress,
        drag: { y: { value: field, min: props.min, max: props.max, unit: valueUnit === "pan" ? undefined : valueUnit, label: props.label } },
        children: [Label(shown)],
      });
    }
    // Steps from where it was pressed, of `values` or of `step`.
    const [pressed, press] = signal(0);
    const drag = now.drag(key);
    const steps = drag.dy.over(-per);
    const dragged = (): Live => {
      if (values) {
        const index = steps.plus(pressed()).clamp(0, values.length - 1);
        return values.reduce<Live>((sum, each, at) => sum.plus(index.eq(at).times(each)), index.times(0));
      }
      const moved = steps.times(step).plus(pressed());
      const above = props.min === undefined ? moved : moved.max(props.min);
      return props.max === undefined ? above : above.min(props.max);
    };
    return Space({
      key, width, height, cursor: "ns-resize",
      onPress: (pointer) => {
        press(values ? Math.max(values.indexOf(taken(get())), 0) : get());
        onPress(pointer);
      },
      drag: {
        id: key, axis: "y", snap: { y: per },
        onEnd: ({ dy }) => {
          const moved = -Math.round(dy / per);
          const next = values ? values[Math.min(Math.max(pressed() + moved, 0), values.length - 1)] : clamped(pressed() + moved * step);
          if (next !== undefined && next !== get()) set(next);
        },
      },
      children: [
        Label({ ...shown, opacity: drag.active.negate().plus(1) }),
        Label({ ...text, text: dragged, unit: valueUnit, opacity: drag.active }),
      ],
    });
  };
  return Show({
    when: typing,
    children: () => {
      const before = untrack(get);
      const done = (typed: string | null) => {
        type(false);
        const number = typed === null ? null : typedNumber(typed, unit);
        if (number !== null && taken(number) !== before) set(taken(number));
      };
      const input = Input({ value: Number.isFinite(before) ? String(Number(before.toFixed(3))) : "-inf", width, focus: true, onCommit: done, onCancel: () => done(null) });
      return h("div", { key: `${key}-typed` }, input);
    },
    fallback: box,
  });
}

/** A number field as a `NumberBox` reads and sets it: -inf as `"-inf"`, as a fader holds it. */
function numberField(path: string, label?: string) {
  const field = valueOf<unknown>({ field: path });
  return {
    get: () => {
      const found = field.get();
      return found === "-inf" ? -Infinity : typeof found === "number" ? found : Number.NaN;
    },
    set: (number: number) => field.set(number === -Infinity ? "-inf" : Number(number.toFixed(3)), label),
  };
}

/** `"bass/synth.cutoff_hz"` as the child `bass/synth` and its field `cutoff_hz`. */
export function splitPath(path: string): { child: string | undefined; name: string } {
  const dot = path.lastIndexOf(".");
  return dot < 0 ? { child: undefined, name: path } : { child: path.slice(0, dot), name: path.slice(dot + 1) };
}

/** The record of the child at `path` below `top`, or `top` itself. */
function recordAt(top: { state: unknown; children: unknown }, path: string | undefined): { state: Record<string, unknown> } | undefined {
  let at = top as { state: Record<string, unknown>; children: readonly { name: string; state: Record<string, unknown>; children: unknown }[] };
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

/** `onChange` hears a pick of another option. */
export type SelectProps<T> = ValueProps<T> & {
  /** Names it for tests: `div-<key>`. */
  key?: string;
  options: Bind<readonly SelectOption<T>[]>;
  /** The heading over its list. */
  title?: string;
  /** What it says while `value` is none of the options: the value when left out. */
  placeholder?: string;
  /** Of its list, in points: 160 when left out. */
  menuWidth?: number;
  /** `"md"` says it in 13 points; 12 when left out, as a select of a card. */
  size?: "sm" | "md";
};

const TRIGGER = {
  sm: { style: { direction: "row", align: "center", gap: 4, height: 24, paddingLeft: 8, paddingRight: 6, radius: 6, background: "alpha/5", hover: { background: "alpha/10" } }, text: 12 },
  md: { style: { direction: "row", align: "center", gap: 6, height: 24, paddingLeft: 9, paddingRight: 8, radius: 6, background: "alpha/5", hover: { background: "alpha/10" } }, text: 13 },
} as const satisfies Record<string, { style: Style; text: number }>;

/** Shows the option `value` is and opens the list of `options` under it, as the window's selects. */
export function Select<const T extends string | number | boolean | null>(props: SelectProps<T>): UiNode {
  const { openMenu } = drawing();
  const { get, set } = valueOf(props);
  const look = TRIGGER[props.size ?? "sm"];
  const said = () => {
    const value = get();
    return read(props.options).find((option) => option.value === value)?.label ?? props.placeholder ?? String(value);
  };
  const open = async () => {
    const [value, options] = [get(), read(props.options)];
    const items = options.map(({ label, detail, icon, disabled, value: each }) => ({ label, detail, icon, disabled, checked: each === value, group: props.title }));
    const picked = await openMenu(items, { width: props.menuWidth ?? 160 });
    const option = picked === null ? undefined : options[picked];
    if (option && option.value !== value) set(option.value);
  };
  return h("div", { key: props.key, style: look.style, onClick: open },
    h("div", { style: { fontSize: look.text, weight: "medium", color: "gray-950" } }, said),
    Icon({ name: "chevron-down", size: 12, color: "gray-700" }),
  );
}

export type SegmentedControlProps<T> = ValueProps<T> & {
  /** Names it for tests: `segmented-<key>`, and each option `segment-<place>`. */
  key?: string;
  options: Bind<readonly Pick<SelectOption<T>, "value" | "label">[]>;
  disabled?: Bind<boolean>;
};

/** One of a few options side by side, as the window's pages of a card. */
export function SegmentedControl<const T extends string | number | boolean | null>(props: SegmentedControlProps<T>): UiNode {
  const { get, set } = valueOf(props);
  const options = () => read(props.options);
  return element("segmented", {
    key: props.key,
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
}

export type SliderProps = { field: string; key?: string; min: number; max: number; default?: number; unit?: Unit; label?: string; played?: boolean; bipolar?: boolean; width?: number };

/**
 * A field or a number input on a short line, as the window's sliders: a drag sideways, ten times
 * finer with shift, a double click to `default`, the arrows step. From `min` to `max`, on a log
 * scale for `"hz"`; `bipolar` draws from 0. 120 points wide unless `width` says.
 */
export function Slider({ field, played, ...props }: SliderProps): UiNode {
  return element("slider", { ...props, field, value: now.value(field, { played }) }, []);
}

/**
 * A quiet line for what the composer should know, as the window's: `tone` is `error` when left
 * out, `progress` runs from 0 to 1.
 */
export const Notice = ({ action, onDismiss, ...props }: { key?: string; message: Bind<string>; tone?: Bind<"error" | "warning" | "info">; progress?: Bind<number>; action?: { label: Bind<string>; onClick: () => void }; onDismiss?: () => void }): UiNode =>
  element("notice", { ...props, action: action?.label, onClick: action?.onClick, onClose: onDismiss }, []);

const STEP = { size: 14, gap: 3 } as const;

export type StepsProps = ValueProps<readonly number[]> & { key?: string; max?: number; playing?: string };

/**
 * A row of steps on a list of numbers, such as a `pattern` field, one per number: a click turns
 * a step on (to `max`, the item's max or 1 when left out) or off (to its min, or 0). `playing`
 * names an output, as `now.value` does, whose value is the step that plays, which lights up.
 */
export function Steps(props: StepsProps): UiNode {
  const field = props.field !== undefined ? drawing().fields[props.field] : undefined;
  const item = field?.kind === "list" && field.item.kind === "number" ? field.item : undefined;
  const [off, on] = [item?.min ?? 0, props.max ?? item?.max ?? 1];
  const { get, set } = valueOf(props);
  const list = () => get() ?? [];
  const isOn = (step: number) => (list()[step] ?? off) > off;
  const pitch = STEP.size + STEP.gap;
  return Space({
    key: props.key ?? (props.field !== undefined ? `steps-${props.field}` : undefined),
    width: () => Math.max(list().length * pitch - STEP.gap, 0),
    height: STEP.size,
    children: For({
      each: () => list().map((_, step) => step),
      children: (step) =>
        Rect({
          x: step * pitch,
          y: 0,
          width: STEP.size,
          height: STEP.size,
          radius: 3,
          color: () => (isOn(step) ? "lavender" : "alpha/10"),
          border: "gray-950",
          borderWidth: props.playing === undefined ? 0 : now.value(props.playing).floor().eq(step).times(2),
          cursor: "pointer",
          onPress: () => {
            const wasOn = isOn(step);
            const changed = [...untrack(list)];
            changed[step] = wasOn ? off : on;
            set(changed, wasOn ? "Turn a step off" : "Turn a step on");
          },
        }),
    }),
  });
}

/** A bar that shows a number from 0 to 1: one named as `now.value` names it, such as a level an output sends, or any live value. */
export function Meter(props: { value: string | Bind<Num>; label?: string }): UiNode {
  const level = () => {
    const value = read(props.value);
    return (typeof value === "string" ? now.value(value) : liveOf(value)).clamp(0, 1).times(64);
  };
  const bar = Space({
    width: 8,
    height: 64,
    children: [
      Rect({ x: 0, y: 0, width: 8, height: 64, radius: 2, color: "alpha/5" }),
      Rect({ x: 0, y: () => level().negate().plus(64), width: 8, height: level, radius: 2, color: "green" }),
    ],
  });
  return h("div", { style: { align: "center", gap: 4 } }, bar, props.label && h("div", { style: { color: "gray-800", fontSize: 11 } }, props.label));
}

/** Fields or number inputs named as `now.value` names them, or what code keeps, from 0 to 1 across and up. */
export type PadProps = { key?: string; size?: number } & Either<{ field: { x: string; y: string } }, Kept<{ x: number; y: number }>>;

/** A square to play with the pointer: across moves `x` and up moves `y`. */
export function Pad(props: PadProps): UiNode {
  const size = props.size ?? 120;
  const look = { width: size, height: size, background: "alpha/5", radius: 6, cursor: "crosshair" } as const;
  const dot = (x: Bind<Num>, y: Bind<Num>) => Circle({ x, y, radius: 5, color: "gray-950" });
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
  const key = props.key ?? controlId("pad");
  const drag = now.drag(key);
  /** Where the pointer is while the drag goes on and until code answers its end, else what code keeps. */
  const at = (pointer: Live, kept: number) => drag.active.times(pointer.clamp(0, size)).plus(drag.active.negate().plus(1).times(kept * size));
  const ended = ({ x, y }: { x: number; y: number }) => onChange({ x: within(x / size), y: within(1 - y / size) });
  return Space({
    ...look,
    key,
    drag: { id: key, onEnd: ended },
    onClick: ended,
    children: [dot(() => at(drag.x, read(value).x), () => at(drag.y, 1 - read(value).y))],
  });
}

/** `tone_hz` is `Tone hz`. */
export function words(name: string): string {
  const spaced = name.replaceAll("_", " ");
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

/**
 * The view of a tool that has none: a knob per number and per number input, a row of steps per
 * list of numbers, a chooser per file, options side by side per choice, a switch per boolean and
 * a button per bang.
 */
export function defaultView(fields: Fields, inputs: Inputs = {}) {
  return ({ state, update, send, openFile }: Context<Record<string, unknown>>): UiNode => {
    const knobs: Child[] = [];
    const rows: Child[] = [];
    // Booleans and bangs share one row, so a card fits its room.
    const buttons: Child[] = [];
    const knob = (name: string, field: NumberField) =>
      Knob({ field: name, label: field.label ?? words(name), min: field.min, max: field.max, default: field.default ?? field.min, unit: field.unit });
    for (const [name, field] of Object.entries(fields)) {
      const label = field.label ?? words(name);
      if (field.kind === "number") {
        knobs.push(knob(name, field));
      } else if (field.kind === "list" && field.item.kind === "number") {
        rows.push(Steps({ field: name }));
      } else if (field.kind === "file") {
        rows.push(
          h("div", { style: { direction: "row", gap: 8, align: "center" } },
            label,
            h("div", { style: { color: "gray-700" } }, () => (state[name] as string) || "No sound"),
            Button({
              label: "Choose…",
              variant: "subtle",
              size: "xs",
              onClick: async () => {
                const chosen = await openFile({ title: `Choose ${label.toLowerCase()}` });
                if (chosen !== null) update(`Choose ${label.toLowerCase()}`, ({ state: next }) => {
                  next[name] = chosen;
                });
              },
            }),
          ),
        );
      } else if (field.kind === "boolean") {
        buttons.push(
          Toggle({
            label,
            on: () => state[name] as boolean,
            onClick: () => {
              const on = state[name] as boolean;
              update(`Turn ${label.toLowerCase()} ${on ? "off" : "on"}`, ({ state: next }) => {
                next[name] = !on;
              });
            },
          }),
        );
      } else if (field.kind === "choice") {
        rows.push(
          h("div", { style: { direction: "row", gap: 8, align: "center" } },
            h("div", { style: { color: "gray-700" } }, label),
            SegmentedControl({ field: name, options: field.options.map((option) => ({ value: option, label: String(option) })) }),
          ),
        );
      }
    }
    for (const [name, input] of Object.entries(inputs)) {
      if (input.kind === "number") {
        knobs.push(knob(name, input));
      } else {
        buttons.push(Button({ label: input.label ?? words(name), variant: "subtle", size: "sm", onClick: () => (send as (input: string) => void)(name) }));
      }
    }
    if (buttons.length > 0) {
      rows.push(h("div", { style: { direction: "row", gap: 4 } }, buttons));
    }
    // The rest goes beside the knobs, not under them: a card is no taller than its room.
    return h(
      "div",
      { style: { direction: "row", gap: 16 } },
      h("div", { style: { direction: "row", gap: 8 } }, knobs),
      h("div", { style: { gap: 8 } }, rows),
    );
  };
}
