import { h, hsv, mix, now, Picture, Space, tool, Wave, type Declaration } from "./sdk";
import { voice } from "./voice";

const children = { voice, meter: "meter" } as const;
const ACROSS = 320;

tool({
  name: "spectra",
  title: "Spectra",
  when: "You want to see a melody as a spectrum that scrolls by, with its wave over it",
  doc: "Plays its child `voice` and shows it: each frame the spectrum (low at the bottom) enters at the right and moves left; its last sound is a line across the middle.",
  state: {},
  children,
  behaviour: (): Declaration<typeof children> => ({
    connections: [
      { from: { child: "voice" }, to: { child: "meter" } },
      { from: { child: "voice" }, to: { port: "audio" } },
    ],
  }),
  view: ({ size }) => (
    <Space grow>
      <Picture
        x={0}
        y={0}
        width={() => size.width}
        height={() => size.height}
        across={ACROSS}
        pixels={({ x, y, wave, last }) => {
          const level = wave(now.spectrum("meter", { bins: 160 }), y.negate().plus(1));
          const fresh = hsv(level.times(-0.75).plus(0.75), 0.85, level.times(1.3).minus(0.15));
          // Every frame moves one pixel left; the newest column is the spectrum now.
          const before = last(x.plus(1 / ACROSS), y);
          const newest = x.gt(1 - 1 / ACROSS);
          return {
            red: mix(before.red, fresh.red, newest),
            green: mix(before.green, fresh.green, newest),
            blue: mix(before.blue, fresh.blue, newest),
          };
        }}
      />
      <Wave x={0} y={() => size.height * 0.3} width={() => size.width} height={() => size.height * 0.4}
        list={now.scope("meter", { frames: 2048 })} color="#ffffffb0" size={1.5} />
    </Space>
  ),
});
