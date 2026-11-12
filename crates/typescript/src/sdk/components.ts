// Components made of the blocks above.

import { Arc, Circle, ControlSize, h, Icon, IconName, Input, Label, Line, Rect, Space, Text } from "./drawing";
import { Unit } from "./fields";
import { Drawing } from "./host";
import { Drag, DragResult, DragTurns, KeyName, Live, now, Num, Press } from "./live";
import { currentCard, element, Show, UiNode } from "./nodes";
import { Accessor, Bind, signal, untrack } from "./reactivity";
import { Key } from "./tools";

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

/** How a control reads its number: in a unit, a pan as `C`, `25L`, `100R`, or a pitch as its note, `C2` for 36. */
type Reading = Unit | "pan" | "pitch";

/** The unit a drag steps a number in: a pan and a pitch have none. */
const unitOf = (reading?: Reading): Unit | undefined => (reading === "pan" || reading === "pitch" ? undefined : reading);

/** How a knob spreads a number from `min` to `max`, as the window's knobs: on a log scale for `"hz"`. */
const knobTravel = (min: number, max: number, unit?: Reading): Travel => ({ min, max, scale: unit === "hz" && min > 0 ? "log" : "linear" });

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

export type KnobProps = ValueProps<number> & {
  key?: string;
  label?: string;
  /** Its value alone under it, with `label` still naming its undo step: for knobs whose names a column beside them says once. */
  bare?: boolean;
  min: number;
  max: number;
  default: number;
  unit?: Reading;
  played?: boolean;
  bipolar?: boolean;
  size?: "sm" | "md";
};

/**
 * A knob, as the app's knobs look, on a field, a number input or what code keeps, see
 * `ValueProps`. With `played` it moves with a lane. A double click puts it at `default`.
 * `bipolar` draws its arc from the top, as a pan's; `unit: "pan"` reads `C`, `25L`, `100R`, and
 * `"pitch"` the note of a pitch, `C2` for 36.
 * `size: "sm"` is a 28-point dial without text.
 */
export function Knob(props: KnobProps): UiNode {
  const { label, min, max, unit, bipolar } = props;
  const small = props.size === "sm";
  const turns = { min, max, unit: unitOf(unit), default: props.default, label: label && `Change ${label.toLowerCase()}` };
  const { key, value, fraction, drag, onPress } = turned("knob", props, knobTravel(min, max, unit), turns, "y");
  const angle = () => fraction().times(SWEEP).minus(SWEEP / 2);
  const [centreX, centreY, radius, cap] = small ? [14, 14, 11, 7] : [28, 18, 16.75, 10.5];
  const along = (distance: number) => () => {
    const radians = angle().times(2 * Math.PI);
    return [radians.sin().times(distance).plus(centreX), radians.cos().times(-distance).plus(centreY)] as const;
  };
  const named = label !== undefined && !props.bare;
  const texts = small ? [] : [
    ...(named ? [Label({ x: centreX, y: 38, text: label, color: "text-secondary", align: "center" })] : []),
    Label({ x: centreX, y: named ? 54 : 38, text: value, unit, color: "text", align: "center" }),
  ];
  return Space({
    key,
    width: small ? 28 : 56,
    height: small ? 28 : named ? 72 : 56,
    cursor: "ns-resize",
    drag,
    onPress,
    children: [
      Arc({ x: centreX, y: centreY, radius, from: -SWEEP / 2, to: SWEEP / 2, color: "fill-strong", width: 2.5 }),
      Arc({ x: centreX, y: centreY, radius, from: bipolar ? 0 : -SWEEP / 2, to: angle, color: "text", width: 2.5 }),
      Circle({ x: centreX, y: centreY, radius: cap, color: "fill-strong" }),
      Line({ from: along(cap / 3), to: along(cap * 0.76), color: "text", width: 2 }),
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
 * on the fader's scale in dB. A double click puts it at `default`.
 */
export function Fader(props: FaderProps): UiNode {
  const { length, horizontal, meter, min, max, unit } = props;
  const travel: Travel = min !== undefined && max !== undefined ? knobTravel(min, max, unit) : { min: 0, max: 0, scale: "fader" };
  const turns = { min, max, unit, travel: length, default: props.default, label: props.label };
  const { key, fraction, drag, onPress } = turned("fader", props, travel, turns, horizontal ? "x" : "y");
  const bars = meter === undefined ? [] : levelBars(meter, length, along(horizontal, length), 8, 5, 2);
  const at = () => fraction().times(length).minus(3);
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
  unit?: Reading | TickUnit;
  /** A field as it plays, see `now.value`. */
  played?: boolean;
  /** The undo step of a field: "Change <field>" when left out. */
  label?: string;
  width: number;
  /** `md` when left out. A small one says its number in 12 points, the others in 14. */
  size?: ControlSize;
  /** Its height in points where no size fits, such as a readout under a fader. */
  height?: number;
  align?: "left" | "center" | "right";
};

/** The heights of `ControlSize`, for the controls drawn here. */
const CONTROL_HEIGHT = { sm: 24, md: 28, lg: 32 } as const;

/** The semitones of each note letter above C. */
const SEMITONES = { c: 0, d: 2, e: 4, f: 5, g: 7, a: 9, b: 11 } as const;

/** A number that `text` says, or `null`: `-6 dB`, `2k` for 2000 in Hz, `-inf`, `F#3` for a pitch. */
function typedNumber(text: string, unit: NumberBoxProps["unit"]): number | null {
  const plain = text.trim().replace("−", "-");
  const note = unit === "pitch" ? /^([a-g])([#b]?)(-?\d+)$/i.exec(plain) : null;
  if (note) {
    const [, letter = "c", accidental, octave] = note;
    return SEMITONES[letter.toLowerCase() as keyof typeof SEMITONES] + (accidental === "#" ? 1 : accidental === "b" ? -1 : 0) + (Number(octave) + 1) * 12;
  }
  if (/^-\s*inf/i.test(plain)) return -Infinity;
  const value = Number.parseFloat(plain);
  if (!Number.isFinite(value)) return null;
  return unit === "hz" && /\dk/i.test(plain.replace(/\s/g, "")) ? value * 1000 : value;
}

/**
 * A number to drag up and down, or double-click and type, as the transport's tempo is: a field,
 * or what code keeps, such as the tempo map, see `ValueProps`. A field's drag and a typed value
 * are one undo step each; what code keeps hears a value once, at the end of a drag. Tab reaches
 * it: enter types, and the arrows step it.
 */
export function NumberBox(props: NumberBoxProps): h.JSX.Element {
  const { key, unit, width, values, field, size = "md" } = props;
  const height = props.height ?? CONTROL_HEIGHT[size];
  const textSize = size === "sm" ? 12 : 14;
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
  // The line of a label is 1.3 times its size.
  const text = { x, y: Math.round((height - textSize * 1.3) / 2), size: textSize, color: "text", align: props.align } as const;
  const shows = props.field !== undefined ? now.value(props.field, { played: props.played }) : props.show !== undefined ? liveOf(props.show) : () => liveOf(get());
  const shown = { ...text, text: shows, unit };
  const valueUnit = unit === undefined || TICK_UNITS.includes(unit as TickUnit) ? undefined : (unit as Reading);
  const onPress = ({ clicks, button }: Press) => {
    if (button === "left" && clicks === 2) type(true);
  };
  const box = (): UiNode => {
    if (field !== undefined) {
      return Space({
        key, width, height, cursor: "ns-resize", onPress,
        drag: { y: { value: field, min: props.min, max: props.max, unit: unitOf(valueUnit), label: props.label } },
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
  // Enter types a number. On what code keeps, which the window cannot step, the arrows step it
  // as a drag does; a field's arrows are the window's, as a knob's.
  const onKey = ({ key }: Key) => {
    if (key === "enter") return type(true);
    const by = key === "up" ? 1 : -1;
    const next = values
      ? values[Math.min(Math.max(Math.max(values.indexOf(taken(get())), 0) + by, 0), values.length - 1)]
      : clamped(get() + by * step);
    if (next !== undefined && next !== get()) set(next);
  };
  const keys: KeyName[] = field === undefined ? ["up", "down", "enter"] : ["enter"];
  return h("div", { key: `${key}-keys`, tabStop: field === undefined, keys, onKey, style: { radius: "sm", keep: true } }, Show({
    when: typing,
    children: () => {
      const before = untrack(get);
      const done = (typed: string | null) => {
        type(false);
        const number = typed === null ? null : typedNumber(typed, unit);
        if (number !== null && taken(number) !== before) set(taken(number));
      };
      const input = Input({ value: Number.isFinite(before) ? String(Number(before.toFixed(3))) : "-inf", width, size, focus: true, onCommit: done, onCancel: () => done(null) });
      return h("div", { key: `${key}-typed` }, input);
    },
    fallback: box,
  }));
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
  /** `md` when left out. A small one says it in 12 points, the others in 14. */
  size?: ControlSize;
  /** A field above its list that keeps the options whose label holds what is typed. */
  search?: boolean;
};

/** Shows the option `value` is and opens the list of `options` under it, as the window's selects. */
export function Select<const T extends string | number | boolean | null>(props: SelectProps<T>): UiNode {
  const { openMenu } = drawing();
  const { get, set } = valueOf(props);
  const size = props.size ?? "md";
  const said = () => {
    const value = get();
    return read(props.options).find((option) => option.value === value)?.label ?? props.placeholder ?? String(value);
  };
  const open = async () => {
    const [value, options] = [get(), read(props.options)];
    const items = options.map(({ label, detail, icon, disabled, value: each }) => ({ label, detail, icon, disabled, checked: each === value, group: props.title }));
    const picked = await openMenu(items, { width: props.menuWidth ?? 160, search: props.search });
    const option = picked === null ? undefined : options[picked];
    if (option && option.value !== value) set(option.value);
  };
  const style = { direction: "row", align: "center", gap: "xs", height: CONTROL_HEIGHT[size], paddingX: "sm", radius: "md", background: "fill", hover: { background: "fill-strong" } } as const;
  // Medium, as what the window's own controls say.
  const text = size === "sm" ? Text({ variant: "label", tone: "text", style: { weight: "medium" }, children: said }) : Text({ variant: "heading", children: said });
  return h("div", { key: props.key, style, tabStop: true, onClick: open }, text, Icon({ name: "chevron-down", size: 12, color: "text-muted" }));
}

export type SegmentedControlProps<T> = ValueProps<T> & {
  /** Names it for tests: `segmented-<key>`, and each option `segment-<place>`. */
  key?: string;
  options: Bind<readonly Pick<SelectOption<T>, "value" | "label">[]>;
  /** `md` when left out. */
  size?: ControlSize;
  disabled?: Bind<boolean>;
};

/** One of a few options side by side, as the window's pages of a card. */
export function SegmentedControl<const T extends string | number | boolean | null>(props: SegmentedControlProps<T>): UiNode {
  const { get, set } = valueOf(props);
  const options = () => read(props.options);
  return element("segmented", {
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

export type StepsProps = ValueProps<readonly number[]> & {
  key?: string;
  max?: number;
  playing?: string;
  /** `sm` when left out, for a pattern in a `Cell`; `md` for steps a trackpad hits. */
  size?: "sm" | "md";
  /** What the row is, at its left. Rows with a label, one over the other, line up. */
  label?: string;
};

/**
 * A row of steps on a list of numbers, such as a `pattern` field, one per number, in groups of
 * four: a click turns a step on (to `max`, the item's max or 1 when left out) or off (to its min,
 * or 0). Tab reaches it: the arrows move along it, and space or enter turns a step. `playing`
 * names an output, as `now.value` does, whose value is the step that plays, which a green ring shows.
 */
export function Steps(props: StepsProps): UiNode {
  const field = props.field !== undefined ? drawing().fields[props.field] : undefined;
  const item = field?.kind === "list" && field.item.kind === "number" ? field.item : undefined;
  const [off, on] = [item?.min ?? 0, props.max ?? item?.max ?? 1];
  const { get, set } = valueOf(props);
  const list = () => get() ?? [];
  return element("steps", {
    key: props.key ?? (props.field !== undefined ? `steps-${props.field}` : undefined),
    size: props.size,
    label: props.label,
    on: () => list().map((value) => value > off),
    playing: props.playing === undefined ? undefined : now.value(props.playing),
    onChange: (step: number) => {
      const changed = [...untrack(list)];
      const wasOn = (changed[step] ?? off) > off;
      changed[step] = wasOn ? off : on;
      set(changed, wasOn ? "Turn a step off" : "Turn a step on");
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
    return (typeof value === "string" ? now.value(value) : liveOf(value)).clamp(0, 1).times(64);
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
