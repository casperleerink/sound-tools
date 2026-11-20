// How a control that drags one number spreads it over its travel, and how the number reads and
// is typed: typed text is read in what the control shows.
import { Live, type Unit } from "../sdk";

/**
 * `log` spreads it in octaves, as a frequency; `fader` is a volume in dB as a mixer's fader
 * spreads it: silence, -inf, at the bottom, 0 dB at 0.8 of the travel and +6 dB at the top.
 * `step`: whole steps from `min`.
 */
export type Scale = "linear" | "log" | "fader";
export type Range = { min: number; max: number; scale?: Scale; step?: number };

/** The fader's law, as the old app's: a power law on the amplitude, 0 dB at 0.8 of the travel, ten times the amplitude every `decade` dB, +6 dB at the top. */
const FADER = { unity: 0.8, decade: 61.94, top: 6 } as const;
/** What a volume fader takes: silence to +6 dB, in tenths of a dB. */
export const VOLUME: Range = { min: -Infinity, max: FADER.top, scale: "fader" };
/** Silence where a number cannot hold -inf: a lane's bottom, and what a fader plays while it drags there. */
export const SILENT_DB = -100;

const clamp = (at: number) => Math.min(Math.max(at, 0), 1);
/** What an arrow key moves of the travel when there is no `step`. */
const KEY_STEP = 0.02;
/** `value` without the noise of binary fractions: 0.1 * 3 is 0.3, and 3e-7 stays 3e-7. */
const clean = (value: number) => Number(value.toPrecision(12));

/** Where `value` is on the travel: 0 at `min`, 1 at `max`, and past them beyond, but a fader's, which is the same whatever its ends and stops at its top. */
export const positionOf = (value: number, { min, max, scale }: Range): number => {
  if (scale === "fader") return Math.min(FADER.unity * 10 ** (value / FADER.decade), 1);
  return scale === "log" ? Math.log(value / min) / Math.log(max / min) : (value - min) / (max - min || 1);
};
/** The value at `position` of the travel, also past its ends: a fader gives -Infinity at 0. */
export const valueOf = (position: number, { min, max, scale }: Range): number => {
  if (scale === "fader") return position <= 0 ? -Infinity : Math.min(FADER.decade * Math.log10(position / FADER.unity), FADER.top);
  return scale === "log" ? min * (max / min) ** position : min + position * (max - min);
};

/** Where `value` is on the travel, 0 to 1. */
export const travelled = (value: number, range: Range): number => clamp(positionOf(value, range));

/** The highest whole step from `min` that is not past `max`. */
const top = ({ min, max, step }: Range) => (step && Number.isFinite(min) ? min + Math.floor((max - min) / step + 1e-9) * step : max);

/** `value` in whole `step`s from `min`, within the range: what every `set`, key and typed value goes through. A fader's is in tenths of a dB. */
export const fitted = (value: number, range: Range): number => {
  const { min, step, scale } = range;
  const stepped = scale === "fader" && Number.isFinite(value) ? Math.round(value * 10) / 10 : step && Number.isFinite(value) ? clean(min + Math.round((value - min) / step) * step) : Number.isFinite(value) ? clean(value) : value;
  return Math.min(Math.max(stepped, min), top(range));
};

/** `value` moved `by` arrow keys: a `step` each, else a fiftieth of the travel. */
export const stepped = (value: number, by: number, range: Range): number =>
  range.step ? fitted(value + by * range.step, range) : valueAt(travelled(value, range) + by * KEY_STEP, range);

/** The value at `at` on the travel. */
export const valueAt = (at: number, range: Range): number => fitted(valueOf(clamp(at), range), range);

/** {@link travelled} of a live value, such as what a lane plays. */
export const liveTravelled = (value: Live, { min, max, scale }: Range): Live => {
  if (scale === "fader") return Live.of(10).pow(value.over(FADER.decade)).times(FADER.unity).clamp(0, 1);
  // `decibels` is 20 log10: a ratio in dB over the range's, a place on a log scale.
  return (scale === "log" ? value.over(min).decibels().over(20 * Math.log10(max / min)) : value.minus(min).over(max - min || 1)).clamp(0, 1);
};

/** Where an amplitude, such as a meter's peak, is on a fader: a level beside it reads on its scale. */
export const liveLevel = (amplitude: Live): Live => amplitude.pow(20 / FADER.decade).times(FADER.unity).min(1);


/** {@link valueAt} as a live value, for the sound while a drag goes on: what a release there sets, a fader's -inf as -100 dB. */
export const liveValueAt = (at: Live, range: Range): Live => {
  const { min, max, scale, step } = range;
  if (scale === "fader") return at.over(FADER.unity).decibels().times(FADER.decade / 20).min(max).times(10).round().over(10).max(SILENT_DB);
  const value = scale === "log" ? Live.of(max / min).pow(at).times(min) : at.times(max - min).plus(min);
  if (!step) return value;
  // A live value's numbers are 32-bit: a step below 1 as a whole number of them per unit, so
  // -96 + 960 / 10 is 0, where 960 * 0.1 would leave a millionth.
  const per = Math.round(1 / step);
  const whole = step < 1 && Math.abs(per * step - 1) < 1e-9;
  const steps = value.minus(min).over(step).round();
  return (whole ? steps.over(per) : steps.times(step)).plus(min).clamp(min, top(range));
};

/** {@link liveValueAt} as a readout shows it: a fader at its bottom says -inf, as a release there sets. */
export const liveShownAt = (at: Live, range: Range): Live => {
  // The window shows a number as 32-bit, which 1e60 is past: -inf. 0 times 1e30 stays 0.
  if (range.scale === "fader") return liveValueAt(at, range).minus(at.le(0).times(1e30).times(1e30));
  if (range.scale === "log" || range.step) return liveValueAt(at, range);
  // On a 100,000th of the range, so 0 of -48 to 24 dB, which 32 bits leave at 0.0000014, reads 0.
  const grain = (range.max - range.min) / 1e5;
  return liveValueAt(at, range).over(grain).round().times(grain);
};

/** Three significant digits, no zeros at the end, and a minus sign: as the window shows a number. */
const short = (value: number): string => {
  if (value === 0) return "0";
  const decimals = Math.max(0, 2 - Math.floor(Math.log10(Math.abs(value))));
  return String(Number(value.toFixed(decimals))).replace("-", "−");
};

const NOTES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/** A MIDI pitch as its note, `C4` for 60, with its cents where it is not whole: `D2 +30`. */
const note = (pitch: number): string => {
  const whole = Math.round(pitch);
  const cents = Math.round((pitch - whole) * 100);
  const name = `${NOTES[((whole % 12) + 12) % 12]}${Math.floor(whole / 12) - 1}`;
  return cents === 0 ? name : `${name} ${cents > 0 ? "+" : "−"}${Math.abs(cents)}`;
};

/** A note as typed, `C4`, `c#3`, `Bb2`, `D2 +30` with cents: its MIDI pitch, or undefined. */
const pitchOf = (typed: string): number | undefined => {
  const match = /^([a-g])(#|b)?(-?\d+)(?:\s*([+-]\d+))?$/.exec(typed);
  if (!match) return undefined;
  const base = NOTES.indexOf(match[1]!.toUpperCase());
  const shift = match[2] === "#" ? 1 : match[2] === "b" ? -1 : 0;
  return (Number(match[3]) + 1) * 12 + base + shift + Number(match[4] ?? 0) / 100;
};

/** `value` as the window shows it in `unit`, such as `2 kHz`, `25L` or `C4`. */
export const shown = (value: number, unit?: Unit): string => {
  if (value === -Infinity && unit === "db") return "−inf dB";
  switch (unit) {
    case "pitch": return note(value);
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
 * `30%`, `25L`, `C`, `C4`, `+3`. A bare number is in one unit per kind, as in other music apps:
 * Hz, ms (also for a field in seconds), dB, percent, a MIDI note, as is; a typed unit wins. A
 * pan's side gives its direction, with no sign. Undefined when it is no value of `unit`.
 */
export const typedValue = (text: string, unit?: Unit): number | undefined => {
  const typed = text.trim().toLowerCase().replace("−", "-");
  if (unit === "pan" && typed === "c") return 0;
  if (unit === "db" && /^-\s*inf/.test(typed)) return -Infinity;
  if (unit === "pitch" && /^[a-g]/.test(typed)) return pitchOf(typed);
  const match = NUMBER.exec(typed);
  if (!match) return undefined;
  const [number, word = ""] = [match[1]!, match[2]];
  if (unit === "pan" && word !== "" && /^[+-]/.test(number)) return undefined;
  const times = WORDS[unit ?? "none"][word];
  return times === undefined ? undefined : Number(number) * times;
};

/** The value typed over `value`, within the range; undefined when it says what was shown, or no value. */
export const typedOver = (text: string, value: number, range: Range, unit?: Unit): number | undefined => {
  if (text.trim() === shown(value, unit)) return undefined;
  const typed = typedValue(text, unit);
  return typed === undefined ? undefined : fitted(typed, range);
};
