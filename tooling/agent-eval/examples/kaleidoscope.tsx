import {
  abs, clamp, hold, channel, cos, delay, exp, feedback, floor, h, list, lowpass, max, min, mix, noise,
  now, number, phasor, Picture, PI, pow, rise, sampleRate, saturate, saw, sin, sine, smooth, Space,
  sqrt, TAU, tool, type Operand, type Signal,
} from "./sdk";

const VIDEO = "zoom.mp4";

/** The angle of a point around the middle, -PI to PI, without atan2: a close polynomial. */
const angleOf = (x: Signal, y: Signal): Signal => {
  const [ax, ay] = [abs(x), abs(y)];
  const a = min(ax, ay).over(max(max(ax, ay), 1e-6));
  const s = a.times(a);
  let r = s.times(-0.0464964749).plus(0.15931422).times(s).minus(0.327622764).times(s).times(a).plus(a);
  r = mix(r, r.negate().plus(PI / 2), ay.gt(ax));
  r = mix(r, r.negate().plus(PI), x.lt(0));
  return mix(r, r.negate(), y.lt(0));
};

/** Folds any place back into 0..1 like a mirror, so the picture never runs off its edge. */
const fold = (v: Signal): Signal => abs(v.mod(2).minus(1)).negate().plus(1);

/** A soft comb: an echo that feeds back through a lowpass, for the room around everything. */
const comb = (x: Operand, ms: Operand, gain: number, darkHz: number): Signal => {
  const loop = feedback();
  const out = delay(lowpass(loop, darkHz), ms, 2000);
  loop.set(out.times(gain).plus(x));
  return out;
};

tool({
  name: "kaleidoscope",
  title: "Kaleidoscope",
  when: "You want a video to loop full screen through a kaleidoscope and play the music itself",
  doc:
    "The video zoom.mp4 loops over the whole window, folded into `segments` mirrored slices that turn at `spin`. " +
    "The picture plays the sound: a pad holds the `chord`, and its filter opens as the video gets brighter, from closed at `dark` to wide open at `light`. " +
    "When much of the picture changes at once, more than `motion_threshold`, a sparkle rings out: bell notes from `sparkle_notes`, faster the more it moves, with echoes. " +
    "`pad_level` and `sparkle_level` balance the two, `space` is how much room is around them.",
  state: {
    segments: number({ min: 2, max: 16, default: 6, integer: true, label: "Segments" }),
    spin: number({ min: -1, max: 1, default: 0.05, label: "Spin" }),
    dark: number({ min: 0, max: 1, default: 0.1, label: "Dark" }),
    light: number({ min: 0, max: 1, default: 0.6, label: "Light" }),
    motion_threshold: number({ min: 0.005, max: 0.3, default: 0.06, label: "Motion" }),
    pad_level: number({ min: 0, max: 1, default: 0.6, unit: "percent", label: "Pad" }),
    sparkle_level: number({ min: 0, max: 1, default: 0.5, unit: "percent", label: "Sparkle" }),
    space: number({ min: 0, max: 1, default: 0.4, unit: "percent", label: "Space" }),
    chord: list(number({ min: 24, max: 96, integer: true }), { max: 5, default: [50, 57, 62, 64, 69] }),
    sparkle_notes: list(number({ min: 48, max: 108, integer: true }), {
      max: 12,
      default: [74, 76, 78, 81, 83, 86, 88, 90],
    }),
  },
  inputs: {
    // Played by the picture every frame: how bright the video is, and how much changed.
    bright: number({ min: 0, max: 1, default: 0 }),
    motion: number({ min: 0, max: 1, default: 0 }),
  },
  sound: ({ bright, motion, dark, light, motion_threshold, pad_level, sparkle_level, space, chord, sparkle_notes }) => {
    // --- Pad: five notes of two detuned saws each, through a filter the brightness opens.
    const side = channel.times(2).minus(1); // -1 left, 1 right
    let pad: Signal = noise().times(0);
    for (let i = 0; i < 5; i++) {
      const hz = pow(2, chord.at(i).minus(69).over(12)).times(440);
      const drift = sine(0.07 + i * 0.031).times(0.004).plus(side.times(0.003 + i * 0.0007));
      pad = pad.plus(saw(hz.times(drift.plus(1)))).plus(saw(hz.times(drift.negate().plus(1).minus(0.002))));
    }
    const amount = clamp(bright.minus(dark).over(max(light.minus(dark), 0.01)), 0, 1);
    const cutoff = smooth(pow(amount, 1.6).times(6800).plus(160), 120);
    const breathe = sine(0.11).times(0.15).plus(1);
    pad = lowpass(lowpass(pad, cutoff.times(breathe), 1.3), cutoff.times(1.5)).times(0.07).times(pad_level);

    // --- Sparkle: a bell note each time the picture changes a lot, repeating while it keeps moving.
    const over = motion.gt(motion_threshold);
    const rate = clamp(motion.over(motion_threshold), 1, 4).times(3);
    const trigger = max(rise(over), rise(phasor(rate).minus(0.5)).times(over));
    const index = floor(hold01(noise(), trigger).times(sparkle_notes.length));
    const f = pow(2, sparkle_notes.at(index).minus(69).over(12)).times(440);
    const env = feedback();
    const level = env;
    env.set(max(trigger, level.times(exp(pow(sampleRate.times(0.35), -1).negate()))));
    const bell = sine(f).plus(sine(f.times(2.76)).times(0.25).times(level)).plus(sine(f.times(5.4)).times(0.08).times(level));
    let sparkle = bell.times(level.times(level)).times(0.18).times(sparkle_level);
    const echo = feedback();
    const echoed = delay(echo, mix(375, 500, channel), 1000);
    echo.set(sparkle.plus(lowpass(echoed, 4000).times(0.45)));
    sparkle = sparkle.plus(echoed.times(0.5));

    // --- Room around both.
    const dry = pad.plus(sparkle);
    const send = lowpass(dry, 5000).times(0.25);
    const odd = channel.times(7);
    const room = comb(send, odd.plus(29.7), 0.82, 3500)
      .plus(comb(send, odd.plus(37.1), 0.8, 3200))
      .plus(comb(send, odd.plus(41.1), 0.78, 3000))
      .plus(comb(send, odd.plus(43.7), 0.76, 2800));
    return saturate(dry.plus(room.times(space.times(0.6)))).times(0.9);
  },
  view: ({ size }) => (
    <Space grow>
      <Picture x={0} y={0} width={() => size.width} height={() => size.height} across={480}
        pixels={({ x, y, aspect, time, live, video, measure }) => {
          // Measure the video itself, not the folded picture, so spinning adds no motion.
          const plain = video(VIDEO);
          const before = video(VIDEO, { at: now.elapsed.minus(0.1) });
          measure("bright", plain.red.times(0.2126).plus(plain.green.times(0.7152)).plus(plain.blue.times(0.0722)));
          measure("motion", abs(plain.red.minus(before.red)).plus(abs(plain.green.minus(before.green)))
            .plus(abs(plain.blue.minus(before.blue))).over(3));

          // The kaleidoscope: fold the angle around the middle into mirrored slices.
          const cx = x.minus(0.5).times(aspect);
          const cy = y.minus(0.5);
          const radius = sqrt(cx.times(cx).plus(cy.times(cy)));
          const wedge = pow(live(now.value("segments")), -1).times(TAU);
          const turned = angleOf(cx, cy).plus(time.times(live(now.value("spin")))).plus(PI * 4);
          const inWedge = turned.mod(wedge);
          const slice = min(inWedge, wedge.minus(inWedge));
          const zoom = sin(time.times(0.13)).times(0.2).plus(1.1);
          const u = fold(cos(slice).times(radius).times(zoom).plus(0.5).plus(sin(time.times(0.05)).times(0.1)));
          const v = fold(sin(slice).times(radius).times(zoom).times(16 / 9).plus(0.5));
          return video(VIDEO, { x: u, y: v });
        }} />
    </Space>
  ),
});

/** Sample and hold, mapped from noise's -1..1 to 0..1 (just under 1, so the last note is reachable but not past it). */
function hold01(x: Signal, when: Signal): Signal {
  return clamp(hold(x, when).plus(1).times(0.5), 0, 0.9999);
}
