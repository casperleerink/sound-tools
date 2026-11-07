import { abs, bandpass, h, hsv, lowpass, max, min, mix, noise, now, number, Canvas, sine, smooth, Space, sqrt, tool } from "./sdk";

const ACROSS = 200;

tool({
  name: "ripples",
  title: "Ripples",
  when: "You want rain on water that you can touch, and hear",
  doc: "Drops fall on a pond, and the pointer stirs it. The small picture is the water itself, the large one its light. The more the water moves, the louder and brighter the rain sounds.",
  state: { rain: number({ min: 0, max: 1, default: 0.4, unit: "percent" }) },
  inputs: { energy: number({ min: 0, max: 1, default: 0 }) },
  sound: ({ energy }) => {
    const moving = smooth(min(energy.times(20), 1), 150);
    const patter = bandpass(noise(), mix(800, 3500, moving), 2).times(moving).times(0.4);
    const under = lowpass(noise(), 300).times(0.15);
    return patter.plus(under).plus(sine(110).times(moving).times(0.05));
  },
  view: ({ size }) => {
    const water = now.space("water");
    return (
      <div style={{ grow: true, direction: "row" }}>
        <Space key="light" grow>
          <Canvas x={0} y={0} width={() => size.width - 240} height={() => (size.width - 240) * (135 / 240)} across={320}
            pixels={({ x, y, canvas }) => {
              const step = 1 / ACROSS;
              const height = (atX: ReturnType<typeof x.plus>) => canvas("water", { x: atX, y }).red;
              const slope = height(x.plus(step)).minus(height(x.minus(step)));
              const level = canvas("water", { x, y }).red.minus(0.5);
              const light = max(0, slope.times(6).plus(0.15));
              const deep = hsv(mix(0.58, 0.52, level.times(2)), 0.7, level.times(0.6).plus(0.35));
              return { red: deep.red.plus(light), green: deep.green.plus(light), blue: deep.blue.plus(light) };
            }} />
        </Space>
        <Space key="water" width={240} height={135}>
          <Canvas key="water" x={0} y={0} width={240} height={135} across={ACROSS}
            pixels={({ x, y, aspect, last, live, measure }) => {
              const [dx, dy] = [1 / ACROSS, aspect.over(ACROSS)];
              const decode = (value: ReturnType<typeof x.plus>) => value.minus(0.5).times(2);
              const here = decode(last().red);
              const before = decode(last().green);
              const around = decode(last(x.plus(dx), y).red)
                .plus(decode(last(x.minus(dx), y).red))
                .plus(decode(last(x, y.plus(dy)).red))
                .plus(decode(last(x, y.minus(dy)).red));
              const wave = around.over(2).minus(before).times(0.985);
              const drop = noise().gt(mix(1, 0.99997, live(now.value("rain"))));
              const [px, py] = [live(water.pointerX.over(240)), live(water.pointerY.over(135))];
              const [ax, ay] = [x.minus(px).times(aspect), y.minus(py)];
              const touched = sqrt(ax.times(ax).plus(ay.times(ay))).lt(0.03).times(live(water.pointerOver));
              const next = mix(wave, 1, max(drop, touched));
              measure("energy", abs(here));
              return { red: next.times(0.5).plus(0.5), green: here.times(0.5).plus(0.5), blue: 0 };
            }} />
        </Space>
      </div>
    );
  },
});
