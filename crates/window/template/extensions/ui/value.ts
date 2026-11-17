// How a control that drags one number spreads it over its travel, and how the number reads and
// is typed: typed text is read in what the control shows.
import { Live, type Unit } from "../sdk";

/** `log` spreads it in octaves, as a frequency. `step`: whole steps from `min`. */
export type Scale = "linear" | "log";
export type Range = { min: number; max: number; scale?: Scale; step?: number };

const clamp = (at: number) => Math.min(Math.max(at, 0), 1);
/** What an arrow key moves of the travel when there is no `step`. */
const KEY_STEP = 0.02;
/** `value` without the noise of binary fractions: 0.1 * 3 is 0.3, and 3e-7 stays 3e-7. */
const clean = (value: number) => Number(value.toPrecision(12));

/** Where `value` is on the travel, 0 to 1. */
export const travelled = (value: number, { min, max, scale }: Range): number =>
  clamp(scale === "log" ? Math.log(value / min) / Math.log(max / min) : (value - min) / (max - min || 1));

/** The highest whole step from `min` that is not past `max`. */
const top = ({ min, max, step }: Range) => (step ? min + Math.floor((max - min) / step + 1e-9) * step : max);

/** `value` in whole `step`s from `min`, within the range: what every `set`, key and typed value goes through. */
export const fitted = (value: number, range: Range): number => {
  const { min, step } = range;
  const stepped = step ? clean(min + Math.round((value - min) / step) * step) : value;
  return Math.min(Math.max(stepped, min), top(range));
};

/** `value` moved `by` arrow keys: a `step` each, else a fiftieth of the travel. */
export const stepped = (value: number, by: number, range: Range): number =>
  range.step ? fitted(value + by * range.step, range) : valueAt(travelled(value, range) + by * KEY_STEP, range);

/** The value at `at` on the travel. */
export const valueAt = (at: number, range: Range): number => {
  const { min, max, scale } = range;
  return fitted(scale === "log" ? min * (max / min) ** clamp(at) : min + clamp(at) * (max - min), range);
};

/** {@link valueAt} as a live value, for what shows while a drag goes on: exactly what a release there sets. */
export const liveValueAt = (at: Live, range: Range): Live => {
  const { min, max, scale, step } = range;
  const value = scale === "log" ? Live.of(max / min).pow(at).times(min) : at.times(max - min).plus(min);
  if (!step) return value;
  // A live value's numbers are 32-bit: a step below 1 as a whole number of them per unit, so
  // -96 + 960 / 10 is 0, where 960 * 0.1 would leave a millionth.
  const per = Math.round(1 / step);
  const whole = step < 1 && Math.abs(per * step - 1) < 1e-9;
  const steps = value.minus(min).over(step).round();
  return (whole ? steps.over(per) : steps.times(step)).plus(min).clamp(min, top(range));
};

/** Three significant digits, no zeros at the end, and a minus sign: as the window shows a number. */
const short = (value: number): string => {
  if (value === 0) return "0";
  const decimals = Math.max(0, 2 - Math.floor(Math.log10(Math.abs(value))));
  return String(Number(value.toFixed(decimals))).replace("-", "−");
};

/** `value` as the window shows it in `unit`, such as `2 kHz` or `25L`. */
export const shown = (value: number, unit?: Unit): string => {
  switch (unit) {
    case "hz": return value < 1000 ? `${short(value)} Hz` : `${short(value / 1000)} kHz`;
    case "ms": return value < 1000 ? `${short(value)} ms` : `${short(value / 1000)} s`;
    case "seconds": return value < 1 ? `${short(value * 1000)} ms` : `${short(value)} s`;
    case "db": return `${short(value)} dB`;
    case "percent": return `${short(value * 100)}%`;
    case "semitones": return `${short(value)} st`;
    case "cents": return `${short(value)} ct`;
    case "octaves": return `${short(value)} oct`;
    case "pan": return value === 0 ? "C" : `${short(Math.abs(value) * 100)}${value < 0 ? "L" : "R"}`;
    default: return short(value);
  }
};

/** The words a typed value may end in, in `unit`, and what each multiplies it by. */
const WORDS: Record<Unit | "none", Record<string, number>> = {
  hz: { "": 1, hz: 1, k: 1000, khz: 1000 },
  ms: { "": 1, ms: 1, s: 1000 },
  seconds: { "": 0.001, s: 1, ms: 0.001 },
  db: { "": 1, db: 1 },
  percent: { "": 0.01, "%": 0.01 },
  semitones: { "": 1, st: 1 },
  cents: { "": 1, ct: 1 },
  octaves: { "": 1, oct: 1 },
  pitch: { "": 1 },
  pan: { "": 0.01, r: 0.01, l: -0.01 },
  none: { "": 1 },
};

const NUMBER = /^([+-]?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?)\s*([a-z%]*)$/;

/**
 * What was typed in `unit`, read as {@link shown} writes it: `2k`, `2 kHz`, `440 Hz`, `1.5 s`,
 * `30%`, `25L`, `C`, `+3`. A bare number is in one unit per kind, whatever shows: Hz, ms, dB,
 * percent, and right of centre for a pan. Undefined when it is no value of `unit`.
 */
export const typedValue = (text: string, unit?: Unit): number | undefined => {
  const typed = text.trim().toLowerCase().replace("−", "-");
  if (unit === "pan" && typed === "c") return 0;
  const match = NUMBER.exec(typed);
  const times = match ? WORDS[unit ?? "none"][match[2] ?? ""] : undefined;
  return match && times !== undefined ? Number(match[1]) * times : undefined;
};

/** The value typed over `value`, within the range; undefined when it says what was shown, or no value. */
export const typedOver = (text: string, value: number, range: Range, unit?: Unit): number | undefined => {
  if (text.trim() === shown(value, unit)) return undefined;
  const typed = typedValue(text, unit);
  return typed === undefined ? undefined : fitted(typed, range);
};
