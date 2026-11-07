import { h, hsv, Knob, max, mix, now, number, output, phasor, Picture, sin, sine, Space, sqrt, Stage, TAU, tool } from "./sdk";

tool({
  name: "glow",
  title: "Glow",
  when: "You want rings of light that breathe with a drone, shown on a second screen",
  doc: "A low drone that swells `speed` times a second; on the stage, rings of colour spread from the middle as it swells and leave trails that fall inwards.",
  state: {
    speed: number({ min: 0.1, max: 4, default: 0.5, unit: "hz" }),
    rings: number({ min: 5, max: 80, default: 40 }),
  },
  sound: ({ speed }) => {
    const swell = sin(phasor(speed).times(TAU)).times(0.5).plus(0.5);
    output("swell", swell);
    return sine(110).plus(sine(164.8).times(0.5)).times(swell).times(0.1);
  },
  view: () => {
    const room = now.space("stage");
    return (
      <div style={{ grow: true, padding: 24, gap: 16 }}>
        <div style={{ direction: "row", gap: 8 }}>
          <Knob field="speed" label="Speed" />
          <Knob field="rings" label="Rings" />
        </div>
        <div style={{ color: "gray-700" }}>The picture plays on the stage.</div>
        <Stage style={{ grow: true }}>
          <Space key="stage" grow>
            <Picture
              x={0}
              y={0}
              width={room.right}
              height={room.bottom}
              across={320}
              pixels={({ x, y, aspect, time, live, last }) => {
                const swell = live(now.value("swell"));
                const across = x.minus(0.5).times(aspect);
                const down = y.minus(0.5);
                const distance = sqrt(across.times(across).plus(down.times(down)));
                const ring = sin(distance.times(live(now.value("rings"))).minus(time.times(3))).times(0.5).plus(0.5);
                const light = ring.times(swell).times(max(0, distance.times(-1.6).plus(1)));
                const colour = hsv(distance.plus(time.times(0.05)), 0.75, light);
                const trail = last(mix(x, 0.5, 0.015), mix(y, 0.5, 0.015));
                return {
                  red: max(colour.red, trail.red.times(0.94)),
                  green: max(colour.green, trail.green.times(0.94)),
                  blue: max(colour.blue, trail.blue.times(0.94)),
                };
              }}
            />
          </Space>
        </Stage>
      </div>
    );
  },
});
