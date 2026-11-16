// Live values: numbers the window works out on every frame, without running code.

/** What a live value is made of, as the window gets it. */
export type LiveNode =
  | { op: "tick" | "elapsed" | "bpm" }
  | { op: "value" | "held"; name: string; child?: string; fraction?: boolean }
  | { op: "peak"; name: string; channel?: Channel }
  | { op: "device_peak"; channel?: Channel }
  | { op: "input"; channel: number }
  | { op: "drag"; id: string; part: "startX" | "startY" | "dx" | "dy" | "x" | "y" | "active" | "alt" }
  | { op: "space"; key: string; part: "left" | "top" | "right" | "bottom" | "pointX" | "pointY" }
  | { op: "hover"; key: string }
  | { op: "sounding"; pitch: number; child?: string }
  | { op: "touched"; name: string }
  | { op: "plus" | "minus" | "times" | "over" | "mod" | "pow" | "min" | "max" | "lt" | "gt" | "le" | "ge" | "eq" | "ne" | "snap"; a: Num; b: Num }
  | { op: "negate" | "abs" | "floor" | "round" | "sin" | "cos" | "decibels"; x: Num }
  | { op: "ease"; x: Num; ease: EaseNode; key: string };

type EaseNode = { kind: "smooth"; ms: number } | { kind: "fall"; per_second: number } | { kind: "hold"; seconds: number };

/** 0 the left channel, 1 the right. */
export type Channel = 0 | 1;

/** A short name of what a live value is made of: the same value drawn twice keeps one memory. */
export function liveKey(value: unknown): string {
  const text = JSON.stringify(value);
  let hash = 2166136261;
  for (let index = 0; index < text.length; index++) {
    hash = Math.imul(hash ^ text.charCodeAt(index), 16777619);
  }
  return (hash >>> 0).toString(36) + text.length.toString(36);
}

/**
 * A number the window works out on every frame, without running code. Combine it with the
 * methods, which take a live value or a number. Code cannot read one: `if (now.tick > 960)` is a
 * type error.
 */
export class Live {
  constructor(readonly node: LiveNode) {}
  /** A number as a live value, to start from: `Live.of(2).pow(octaves)`. */
  static of(value: Num): Live { return typeof value === "number" ? new Live({ op: "plus", a: value, b: 0 }) : value; }
  toJSON(): LiveNode { return this.node; }
  plus(other: Num): Live { return new Live({ op: "plus", a: this, b: other }); }
  minus(other: Num): Live { return new Live({ op: "minus", a: this, b: other }); }
  times(other: Num): Live { return new Live({ op: "times", a: this, b: other }); }
  over(other: Num): Live { return new Live({ op: "over", a: this, b: other }); }
  min(other: Num): Live { return new Live({ op: "min", a: this, b: other }); }
  max(other: Num): Live { return new Live({ op: "max", a: this, b: other }); }
  clamp(low: Num, high: Num): Live { return this.max(low).min(high); }
  /** 1 where the comparison holds, else 0. */
  lt(other: Num): Live { return new Live({ op: "lt", a: this, b: other }); }
  gt(other: Num): Live { return new Live({ op: "gt", a: this, b: other }); }
  le(other: Num): Live { return new Live({ op: "le", a: this, b: other }); }
  ge(other: Num): Live { return new Live({ op: "ge", a: this, b: other }); }
  eq(other: Num): Live { return new Live({ op: "eq", a: this, b: other }); }
  ne(other: Num): Live { return new Live({ op: "ne", a: this, b: other }); }
  /** The nearest whole number of `step`s: `dx.snap(240)` moves by sixteenths. */
  snap(step: Num): Live { return new Live({ op: "snap", a: this, b: step }); }
  negate(): Live { return new Live({ op: "negate", x: this }); }
  abs(): Live { return new Live({ op: "abs", x: this }); }
  floor(): Live { return new Live({ op: "floor", x: this }); }
  round(): Live { return new Live({ op: "round", x: this }); }
  /** Of an angle in radians. */
  sin(): Live { return new Live({ op: "sin", x: this }); }
  cos(): Live { return new Live({ op: "cos", x: this }); }
  /** What is left over after whole `other`s, from 0 up: `now.elapsed.mod(1)` goes 0 to 1 every second. */
  mod(other: Num): Live { return new Live({ op: "mod", a: this, b: other }); }
  pow(exponent: Num): Live { return new Live({ op: "pow", a: this, b: exponent }); }
  /** An amplitude in decibels: 1 is 0 dB; -120 at the lowest. */
  decibels(): Live { return new Live({ op: "decibels", x: this }); }
  /** Glides to it, about two thirds of the way in `ms`. */
  smooth(ms: number): Live { return this.ease({ kind: "smooth", ms }); }
  /** Rises with it at once and falls at most `perSecond` a second: a meter's bar. */
  fall(perSecond: number): Live { return this.ease({ kind: "fall", per_second: perSecond }); }
  /** Rises with it at once and stays `seconds` before it follows it: a meter's peak line, `level.hold(1.5).fall(0.5)`. */
  hold(seconds: number): Live { return this.ease({ kind: "hold", seconds }); }
  private ease(ease: EaseNode): Live { return new Live({ op: "ease", x: this, ease, key: liveKey([this, ease]) }); }
}

/** A number, or a live value the window works out on every frame. */
export type Num = number | Live;

/**
 * `"bass/synth.cutoff_hz"` as the child `bass/synth` and its number `cutoff_hz`: the first dot
 * ends the child, and what follows is where its record holds the number, `pads.36.volume_db`.
 * A name of a child holds no `[`, so `"voices[2].detune"` is a number of the record itself.
 */
export function splitPath(path: string): { child?: string; name: string } {
  const dot = path.indexOf(".");
  return dot < 0 || path.slice(0, dot).includes("[") ? { name: path } : { child: path.slice(0, dot), name: path.slice(dot + 1) };
}
