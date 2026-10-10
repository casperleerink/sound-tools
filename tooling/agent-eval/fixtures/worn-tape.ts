import { choice, db, delay, highpass, input, knob, lowpass, mix, noise, type Signal, sine, tool } from "./sdk";

// Each wobble is [rate in hz, peak pitch change as a ratio], at full wear.
// Two slow ones for wow and two fast ones for flutter, at rates that never line up,
// so the drift does not repeat in an obvious loop.
const CHARACTERS = {
  cassette: {
    wobbles: [[0.55, 0.006], [0.23, 0.003], [9.1, 0.0015], [13.7, 0.0006]],
    // Treble left at no wear and at full wear.
    top_hz: [11000, 4500],
    // Hiss level at no wear and at full wear, and how high it reaches.
    hiss_db: [-58, -34],
    hiss_top_hz: 7000,
  },
  reel: {
    wobbles: [[0.35, 0.002], [0.17, 0.001], [15.3, 0.0005], [21.1, 0.0002]],
    top_hz: [18000, 10000],
    hiss_db: [-68, -46],
    hiss_top_hz: 12000,
  },
} as const;

tool({
  name: "worn-tape",
  title: "Worn tape",
  when: "You want a sound to play as if off an old, worn tape: wow, flutter and hiss",
  doc: [
    "Plays the sound as if off an old tape. The pitch drifts slowly up and down (wow), with a faster, small shake on top (flutter); the treble dulls and tape hiss sits under it.",
    "",
    "- `wear`: how worn the tape is. 0 is a fresh tape: steady pitch, almost no hiss. 1 is a tired one: a seasick wow of about a sixth of a semitone, audible flutter, dull top and clear hiss.",
    "- `character`: `cassette` wobbles more, is darker and hisses more. `reel` is gentler and brighter, with quieter hiss.",
    "",
    "Both channels wobble together, as on one tape; the hiss is apart on each channel. It delays the sound by about 5 ms.",
  ].join("\n"),
  state: {
    wear: knob({ min: 0, max: 1, default: 0.5, label: "Wear" }),
    character: choice({ options: ["cassette", "reel"], default: "cassette", label: "Character" }),
  },
  sound: ({ wear, character }) => {
    const c = CHARACTERS[character];
    // A delay moving by `ms` * sin(2 pi f t) changes the pitch by up to 2 pi f ms / 1000.
    const depths = c.wobbles.map(([hz, ratio]) => [hz, (ratio / (2 * Math.PI * hz)) * 1000] as const);
    const base_ms = depths.reduce((sum, [, ms]) => sum + ms, 0) + 1;
    const wobble = depths
      .map(([hz, ms]): Signal => sine(hz).times(ms))
      .reduce((sum, wave) => sum.plus(wave));
    const played = delay(input, wobble.times(wear).plus(base_ms));
    const dull = lowpass(played, mix(c.top_hz[0], c.top_hz[1], wear));
    const hiss = lowpass(highpass(noise(), 800), c.hiss_top_hz).times(
      db(mix(c.hiss_db[0], c.hiss_db[1], wear)),
    );
    return dull.plus(hiss);
  },
});
