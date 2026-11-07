import { abs, adsr, h, max, mix, noise, now, number, output, phasor, Picture, rise, sin, Space, TAU, tool, wrap } from "./sdk";

tool({
  name: "mirror",
  title: "Mirror",
  when: "You want a video folded into a mirror whose colours split on every beat",
  doc: "A kick `tempo` times a minute. The video `zoom.mp4` shows folded at the middle; each kick pulls its red and blue apart and leaves a trail.",
  state: {
    tempo: number({ min: 60, max: 180, default: 120 }),
  },
  sound: ({ tempo }) => {
    const beat = phasor(tempo.over(60));
    const hit = adsr(rise(wrap(beat).lt(0.1)).plus(wrap(beat).lt(0.1)), 1, 180, 0, 120);
    output("hit", hit);
    const kick = sin(phasor(hit.times(hit).times(120).plus(45)).times(TAU)).times(hit);
    return kick.times(0.4).plus(noise().times(hit).times(0.03));
  },
  view: ({ size }) => (
    <Space grow>
      <Picture
        x={0}
        y={0}
        width={() => size.width}
        height={() => size.height}
        across={320}
        pixels={({ x, y, live, video, last }) => {
          const hit = live(now.value("hit"));
          // Folded at the middle: the right half is the left half seen in a mirror.
          const folded = abs(x.minus(0.5)).negate().plus(0.5).times(2);
          const split = hit.times(0.04);
          const red = video("zoom.mp4", { x: folded.plus(split), y }).red;
          const green = video("zoom.mp4", { x: folded, y }).green;
          const blue = video("zoom.mp4", { x: folded.minus(split), y }).blue;
          const trail = last(mix(x, 0.5, 0.01), mix(y, 0.5, 0.01));
          const keep = hit.times(0.8);
          return {
            red: max(red, trail.red.times(keep)),
            green: max(green, trail.green.times(keep)),
            blue: max(blue, trail.blue.times(keep)),
          };
        }}
      />
    </Space>
  ),
});
