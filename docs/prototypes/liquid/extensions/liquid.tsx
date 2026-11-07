import { adsr, h, lowpass, now, number, output, phasor, Picture, rise, saw, Space, tool, wrap } from "./sdk";

tool({
  name: "liquid",
  title: "Liquid",
  when: "You want a still picture that melts with a pulsing bass",
  doc: "A bass pulses `tempo` times a minute; the photo `fractal.jpg` ripples like liquid, the more the louder the bass is.",
  state: { tempo: number({ min: 60, max: 160, default: 100 }) },
  sound: ({ tempo }) => {
    const beat = wrap(phasor(tempo.over(60)));
    const hit = adsr(rise(beat.lt(0.5)).plus(beat.lt(0.5)), 5, 400, 0, 200);
    output("hit", hit);
    return lowpass(saw(55), hit.times(1200).plus(100), 4).times(hit).times(0.3);
  },
  view: ({ size }) => (
    <Space grow>
      <Picture x={0} y={0} width={() => size.width} height={() => size.height} across={360}
        pixels={({ x, y, time, noise, live, photo }) => {
          const hit = live(now.value("hit"));
          const melt = hit.times(0.06).plus(0.01);
          const bendX = noise(x.times(3), y.times(3), time.times(0.3)).times(melt);
          const bendY = noise(x.times(3).plus(7), y.times(3), time.times(0.3)).times(melt);
          return photo("fractal.jpg", { x: x.plus(bendX), y: y.plus(bendY) });
        }} />
    </Space>
  ),
});
