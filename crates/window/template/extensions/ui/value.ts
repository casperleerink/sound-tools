// How a control that drags one number spreads it over its travel, and how the number is typed.
import { Live, type Unit } from "../sdk";

/** `log` spreads it in octaves, as a frequency. */
export type Scale = "linear" | "log";
export type Range = { min: number; max: number; scale?: Scale; step?: number };

const clamp = (at: number) => Math.min(Math.max(at, 0), 1);

/** Where `value` is on the travel, 0 to 1. */
export const travelled = (value: number, { min, max, scale }: Range): number =>
  clamp(scale === "log" ? Math.log(value / min) / Math.log(max / min) : (value - min) / (max - min || 1));

/** The value at `at` on the travel, in whole `step`s. */
export const valueAt = (at: number, { min, max, scale, step }: Range): number => {
  const value = scale === "log" ? min * (max / min) ** clamp(at) : min + clamp(at) * (max - min);
  return step ? Math.round(value / step) * step : value;
};

/** {@link valueAt} as a live value, for what shows while a drag goes on. */
export const liveValueAt = (at: Live, { min, max, scale, step }: Range): Live => {
  const value = scale === "log" ? Live.of(max / min).pow(at).times(min) : at.times(max - min).plus(min);
  return step ? value.over(step).round().times(step) : value;
};

/** `value` as the composer types it: a percent as 50 for 0.5, a frequency as 2k. */
export const typedText = (value: number, unit?: Unit): string => {
  if (unit === "percent") return `${Math.round(value * 100)}`;
  if (unit === "hz" && value >= 1000) return `${Number((value / 1000).toFixed(2))}k`;
  return `${Number(value.toFixed(2))}`;
};

/** What was typed as a value, or undefined when it is no number. */
export const typedValue = (text: string, unit?: Unit): number | undefined => {
  const match = /^\s*(-?[\d.]+)\s*(k)?/i.exec(text);
  if (!match) return undefined;
  const number = Number(match[1]) * (match[2] ? 1000 : 1);
  if (!Number.isFinite(number)) return undefined;
  return unit === "percent" ? number / 100 : number;
};
