import { clamp, cos, feedback, hold, knob, lookup, max, min, noise, phasor, pow, rise, sample, sampleRate, type Signal, sqrt, TAU, tool, wrap } from "./sdk";

// Grains come from this many streams; each fires in turn, so at most this many overlap.
const STREAMS = 16;
// How far around `position` a grain may start, in seconds, so repeats do not sound the same.
const SCATTER_SECONDS = 0.04;

tool({
  name: "grain-cloud",
  title: "Grain cloud",
  when: "You want a recording sprayed as a cloud of tiny grains: a granular texture that plays by itself",
  doc: [
    "A granular cloud that plays by itself, notes or not: it keeps firing short, softly windowed grains of `sound`, each from near `position` in the file, at a slightly random time, pitch and place in the stereo field.",
    "",
    "- `size`: how long each grain lasts. 10 to 40 ms buzzes and crackles, 60 to 150 ms is a smooth smear, 300 ms and up lets the recording be heard in short phrases. When `density` is so high that a grain would outlast its turn, it is cut shorter.",
    "- `density`: grains a second, on a log scale. Under 5 is sparse, single drops; 20 to 40 a steady cloud; 100 and up a dense wash. The level is evened out, so more grains is thicker, not much louder.",
    "- `position`: where in the file the grains read, 0 the start, 0.5 the middle, 1 the end. Each grain starts up to 40 ms either side of it. Automate it to scan through the recording.",
    "- `pitch_spread`: how far each grain's pitch may stray, in semitones up or down, at random. 0 keeps the recording's pitch, 0.1 to 0.3 a chorus-like shimmer, 12 a cloud over two octaves. Half way is a spread half as wide.",
  ].join("\n"),
  kind: "source",
  state: {
    sound: sample({ label: "Sound" }),
    size: knob({ min: 10, max: 1000, default: 120, unit: "ms", label: "Grain size" }),
    density: knob({ min: 1, max: 200, default: 30, unit: "hz", label: "Density" }),
    position: knob({ min: 0, max: 1, default: 0.5, unit: "percent", label: "Position" }),
    pitch_spread: knob({ min: 0, max: 12, default: 0.5, label: "Pitch spread" }),
  },
  sound: ({ sound, size, density, position, pitch_spread }) => {
    const streamRate = density.over(STREAMS);
    // A grain lasts `size`, but never longer than its stream's turn, in samples.
    const grainLength = max(1, min(size.over(1000).times(streamRate), 1).over(streamRate).times(sampleRate));
    const scatter = sampleRate.times(SCATTER_SECONDS).over(max(1, sound.length));
    // The more grains overlap, the quieter each, so the cloud keeps about one level.
    const level = sqrt(max(1, density.times(size).over(1000))).times(0.7);

    let left: Signal | undefined;
    let right: Signal | undefined;
    for (let stream = 0; stream < STREAMS; stream++) {
      // Each stream fires once a turn, a little early or late at random, offset from the others.
      const jitter = feedback();
      const fire = rise(wrap(phasor(streamRate.times(jitter.times(0.4).plus(1))).plus(stream / STREAMS)).lt(0.5));
      jitter.set(hold(noise(), fire));

      // Samples since the grain began; silent until the stream first fires.
      const count = feedback();
      const elapsed = count.plus(1).times(fire.eq(0));
      count.set(elapsed);
      const armed = feedback();
      const started = max(armed, fire);
      armed.set(started);

      const start = hold(position.plus(noise().times(scatter)), fire);
      const ratio = hold(pow(2, noise().times(pitch_spread).over(12)), fire);
      const pan = hold(noise().times(0.7), fire);

      const progress = clamp(elapsed.over(grainLength), 0, 1);
      const window = cos(progress.times(TAU)).times(-0.5).plus(0.5).times(started);
      const grain = lookup(sound, wrap(start.plus(elapsed.times(ratio).over(max(1, sound.length))))).times(window);

      const toLeft = grain.times(sqrt(pan.negate().plus(1).times(0.5)));
      const toRight = grain.times(sqrt(pan.plus(1).times(0.5)));
      left = left ? left.plus(toLeft) : toLeft;
      right = right ? right.plus(toRight) : toRight;
    }
    return { left: left!.over(level), right: right!.over(level) };
  },
});
