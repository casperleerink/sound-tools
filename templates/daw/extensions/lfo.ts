import { abs, adsr, beat, choice, cos, exp, log, max, note, number, output, tool, wrap, type Signal } from "./sdk";

// What moves a number of a track over time other than a lane: an LFO in time with the piece, and
// an envelope that follows the track's notes. Each sends its value out of `value` between `low`
// and `high`, in the units of the number it moves, as a lane does.

export const RATES = ["1/1", "1/2", "1/4", "1/8", "1/16"] as const;
/** The quarter notes of one cycle of each rate. */
const BEATS: Record<(typeof RATES)[number], number> = { "1/1": 4, "1/2": 2, "1/4": 1, "1/8": 0.5, "1/16": 0.25 };
export const SHAPES = ["sine", "triangle", "square", "saw"] as const;

/** Wide enough for any number a lane moves: a frequency, a volume, a time or a share. */
const range = (fallback: number) => number({ min: -100_000, max: 100_000, default: fallback });
const between = { low: range(0), high: range(1), scale: choice({ options: ["linear", "log"], default: "linear" }) };

/** `amount`, 0 to 1, of the way from `low` to `high`: by equal ratios on a log scale, as a frequency sounds even. */
function along(low: Signal, high: Signal, amount: Signal, scale: "linear" | "log"): Signal {
  if (scale === "linear") return low.plus(high.minus(low).times(amount));
  const [from, to] = [log(max(low, 1e-6)), log(max(high, 1e-6))];
  return exp(from.plus(to.minus(from).times(amount)));
}

/** How an envelope moves: its times and its sustain, each with the name its knob shows. */
export const ENVELOPE = {
  attack_ms: number({ min: 1, max: 5_000, default: 10, unit: "ms", label: "Attack" }),
  decay_ms: number({ min: 1, max: 5_000, default: 200, unit: "ms", label: "Decay" }),
  sustain: number({ min: 0, max: 1, default: 0.5, unit: "percent", label: "Sustain" }),
  release_ms: number({ min: 1, max: 10_000, default: 300, unit: "ms", label: "Release" }),
};

export const lfo = tool({
  name: "lfo",
  title: "LFO",
  when: "A number of a track should sweep up and down in time with the piece",
  doc: "Sends `value`, which goes from `low` to `high` and back once every `rate` (a note length, `1/4` a quarter note) in the shape of `shape`, from `low` on the beat. It follows the beat of the piece, so it holds still while the piece stands. `low` and `high` are in the units of the number it moves; `scale` `log` moves by equal ratios, for a frequency.",
  state: { shape: choice({ options: SHAPES, default: "sine" }), rate: choice({ options: RATES, default: "1/4" }), ...between },
  sound: ({ shape, rate, low, high, scale }) => {
    // Each shape is at `low` on the beat a cycle starts.
    const phase = wrap(beat.over(BEATS[rate]));
    const wave =
      shape === "sine" ? cos(phase.times(2 * Math.PI)).times(-0.5).plus(0.5)
      : shape === "triangle" ? abs(phase.times(2).minus(1)).negate().plus(1)
      : shape === "square" ? phase.ge(0.5)
      : phase;
    output("value", along(low, high, wave, scale));
    return 0;
  },
});

export const envelope = tool({
  name: "envelope",
  title: "Envelope",
  when: "A number of a track should rise with each note it plays and fall after",
  doc: "Sends `value`, which is `low` with no note, rises to `high` over `attack_ms` when a note starts, falls to `sustain` (0 to 1 of the way) over `decay_ms` while it is held, and back to `low` over `release_ms` after it. It follows the newest note of the track. `low` and `high` are in the units of the number it moves; `scale` `log` moves by equal ratios, for a frequency.",
  state: { ...ENVELOPE, ...between },
  sound: ({ attack_ms, decay_ms, sustain, release_ms, low, high, scale }) => {
    output("value", along(low, high, adsr(note.gate, attack_ms, decay_ms, sustain, release_ms), scale));
    return 0;
  },
});
