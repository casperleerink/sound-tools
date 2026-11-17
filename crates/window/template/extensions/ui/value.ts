// How a control that drags one number spreads it over its travel, and how the number reads and
// is typed: typed text is read in what the control shows.
import { Live, type Unit } from "../sdk";

/** `log` spreads it in octaves, as a frequency. `step`: whole steps from `min`. */
export type Scale = "linear" | "log";
export type Range = { min: number; max: number; scale?: Scale; step?: number };

const clamp = (at: number) => Math.min(Math.max(at, 0), 1);

/** Where `value` is on the travel, 0 to 1. */
export const travelled = (value: number, { min, max, scale }: Range): number =>
  clamp(scale === "log" ? Math.log(value / min) / Math.log(max / min) : (value - min) / (max - min || 1));

/** The highest whole step from `min` that is not past `max`. */
const top = ({ min, max, step }: Range) => (step ? min + Math.floor((max - min) / step + 1e-9) * step : max);

/** `value` in whole `step`s from `min`, within the range. */
export const fitted = (value: number, range: Range): number => {
  const { min, step } = range;
  const stepped = step ? min + Math.round((value - min) / step) * step : value;
  return Math.min(Math.max(stepped, min), top(range));
};

/** The value at `at` on the travel. */
export const valueAt = (at: number, range: Range): number => {
  const { min, max, scale } = range;
  return fitted(scale === "log" ? min * (max / min) ** clamp(at) : min + clamp(at) * (max - min), range);
};

/** {@link valueAt} as a live value, for what shows while a drag goes on. */
export const liveValueAt = (at: Live, range: Range): Live => {
  const { min, max, scale, step } = range;
  const value = scale === "log" ? Live.of(max / min).pow(at).times(min) : at.times(max - min).plus(min);
  return step ? value.minus(min).over(step).round().times(step).plus(min).clamp(min, top(range)) : value;
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
    case "pan": return value === 0 ? "C" : `${short(Math.abs(value) * 100)}${value < 0 ? "L" : "R"}`;
    default: return short(value);
  }
};

/**
 * What was typed in `unit`, read as {@link shown} writes it: `2k`, `2 kHz` and `440 Hz`, `1.5 s`,
 * `30%`, `25L`, `C`. A bare number is in the unit itself, a share in percent. Undefined when it is
 * no number.
 */
export const typedValue = (text: string, unit?: Unit): number | undefined => {
  const typed = text.trim().toLowerCase().replace("−", "-");
  if (unit === "pan" && typed === "c") return 0;
  const match = /^(-?\d*\.?\d+)\s*([a-z%]*)$/.exec(typed);
  if (!match) return undefined;
  const [number, word] = [Number(match[1]), match[2] ?? ""];
  switch (unit) {
    case "hz": return word.startsWith("k") ? number * 1000 : number;
    case "ms": return word === "s" ? number * 1000 : number;
    case "seconds": return word === "ms" ? number / 1000 : number;
    case "percent": return number / 100;
    case "pan": return word === "l" ? -number / 100 : number / 100;
    default: return number;
  }
};
