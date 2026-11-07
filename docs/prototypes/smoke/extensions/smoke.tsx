import { bandpass, exp, h, hsv, mtof, noise, now, number, Picture, sine, smooth, Space, sqrt, tool } from "./sdk";

tool({
  name: "smoke",
  title: "Smoke",
  when: "You want to play a breathy tone by moving the pointer through smoke",
  doc: "Smoke drifts across the window. Where the pointer is, it lights up: left to right plays two octaves up from C3, low to high breathes louder.",
  state: {},
  inputs: {
    pitch: number({ min: 0, max: 1, default: 0.5 }),
    air: number({ min: 0, max: 1, default: 0 }),
  },
  sound: ({ pitch, air }) => {
    const hz = mtof(pitch.times(24).plus(48));
    const breath = bandpass(noise(), hz.times(2), 6).times(0.6);
    const tone = sine(hz.times(sine(5).times(0.004).plus(1)));
    return tone.plus(breath).times(smooth(air, 80)).times(0.25);
  },
  view: () => {
    const room = now.space("smoke");
    return (
      <Space key="smoke" grow>
        <Picture
          x={0}
          y={0}
          width={room.right}
          height={room.bottom}
          across={240}
          pixels={({ x, y, aspect, time, noise, live, measure }) => {
            const [across, down] = [x.times(aspect), y];
            const drift = time.times(0.15);
            const smoke = noise(across.times(3).minus(drift), down.times(3), drift)
              .plus(noise(across.times(7), down.times(7).plus(drift), drift.times(2)).times(0.5))
              .times(0.5).plus(0.5);
            const [pointerX, pointerY] = [live(room.pointerX.over(room.right)), live(room.pointerY.over(room.bottom))];
            const over = live(room.pointerOver);
            const [dx, dy] = [x.minus(pointerX).times(aspect), y.minus(pointerY)];
            const glow = exp(sqrt(dx.times(dx).plus(dy.times(dy))).times(-6)).times(over);
            measure("pitch", pointerX);
            measure("air", over.times(pointerY.negate().plus(1)));
            return hsv(pointerX.times(0.4).plus(0.55).plus(smoke.times(0.1)), 0.55, smoke.times(smoke).times(glow.times(1.6).plus(0.25)));
          }}
        />
      </Space>
    );
  },
});
