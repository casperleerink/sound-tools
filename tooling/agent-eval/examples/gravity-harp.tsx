import { type BangInput, type Color, type ShapeLiteral, type Style, Button, Knob, SegmentedControl, Space, Text, channel, choice, delay, feedback, h, number, lowpass, max, mix, noise, pow, sampleRate, tool, bang } from "./sdk";

// The page, in points.
const WIDTH = 640;
const HEIGHT = 420;
const RADIUS = 7;

// The strings, from the lowest note at the bottom to the highest at the top. They are staggered
// so a ball that rolls off one falls on to another.
const strings = [
  { y: 365, x0: 40, x1: 600, color: "pink" },
  { y: 300, x0: 60, x1: 380, color: "peach" },
  { y: 240, x0: 300, x1: 580, color: "yellow" },
  { y: 180, x0: 70, x1: 310, color: "green" },
  { y: 120, x0: 380, x1: 580, color: "blue" },
] as const satisfies readonly { y: number; x0: number; x1: number; color: Color }[];

const plucks = ["pluck0", "pluck1", "pluck2", "pluck3", "pluck4"] as const;
type Pluck = (typeof plucks)[number];

const roots = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9 } as const;
const scales = { major: [0, 2, 4, 7, 9], minor: [0, 3, 5, 7, 10] } as const;

/** The frequency of each string, low to high. */
function notes(root: keyof typeof roots, scale: keyof typeof scales, octave: number): number[] {
  const c = 12 * (octave + 1);
  return scales[scale].map((step) => 440 * 2 ** ((c + roots[root] + step - 69) / 12));
}

type Ball = { x: number; y: number; vx: number; vy: number; color: Color; age: number };
type Memory = { balls: Ball[]; shake: number[]; time: number };

const DEFAULTS = { gravity: 900, bounce: 0.7 };

const page: Style = { gap: "md", padding: "md", background: "background" };
const row: Style = { direction: "row", gap: "md", align: "center" };

tool({
  name: "gravity-harp",
  title: "Gravity harp",
  when: "You want the gravity harp: balls dropped with the pointer that pluck strings",
  doc:
    "An interactive sound toy. A press on the page drops a ball; it falls on to five horizontal strings, " +
    "and each string it hits is plucked and throws it back up. The strings are tuned to a pentatonic scale, " +
    "lowest at the bottom. `root`, `scale` and `octave` tune them. `decay` is how long a string rings, " +
    "`brightness` how bright the pluck is: low is a soft thumb, high a hard pick. `space` is how much of a " +
    "soft stereo echo comes with it, 0 dry. `gravity` is how fast balls fall and `bounce` how much of its " +
    "speed a ball keeps on each hit: 1 would bounce for ever, the middle settles after a few hits.",
  state: {
    root: choice({ options: ["C", "D", "E", "F", "G", "A"], default: "D" }),
    scale: choice({ options: ["major", "minor"], default: "minor" }),
    octave: choice({ options: [3, 4, 5], default: 4 }),
    decay: number({ min: 0.3, max: 12, default: 4, label: "Decay" }),
    brightness: number({ min: 300, max: 12000, default: 3500, unit: "hz" }),
    space: number({ min: 0, max: 1, default: 0.35, unit: "percent" }),
    volume: number({ min: 0, max: 1, default: 0.5, unit: "percent" }),
    gravity: number({ min: 150, max: 2500, default: DEFAULTS.gravity }),
    bounce: number({ min: 0.2, max: 0.95, default: DEFAULTS.bounce }),
  },
  inputs: Object.fromEntries(plucks.map((name) => [name, bang()])) as Record<Pluck, BangInput>,
  memory: (): Memory => ({ balls: [], shake: strings.map(() => 0), time: 0 }),

  sound: (fields) => {
    const { root, scale, octave, decay, brightness, space, volume } = fields;
    const voices = notes(root, scale, octave).map((freq, i) => {
      // A short burst of noise, filtered by `brightness`, is the pick.
      const pick = feedback();
      pick.set(max(fields[plucks[i]], pick.times(0.993)));
      const excite = lowpass(noise(), brightness).times(pick);

      // Karplus-Strong: a delay one period long, fed back through a two-point average.
      // The loop is the delay, plus one sample of feedback, plus half a sample of the average.
      const loop = feedback();
      const previous = feedback();
      const period = delay(excite.plus(loop), sampleRate.over(freq).minus(1.5).times(1000).over(sampleRate));
      previous.set(period);
      // The gain each time round that leaves 1/1000 (-60 dB) after `decay` seconds.
      const keep = pow(0.001, pow(decay.times(freq), -1));
      loop.set(period.plus(previous).times(0.5).times(keep));

      // Spread the strings across the stereo field, low on the left.
      const pan = 0.2 + (0.6 * i) / (strings.length - 1);
      return period.times(mix(Math.cos((pan * Math.PI) / 2), Math.sin((pan * Math.PI) / 2), channel));
    });
    const dry = voices.reduce((sum, voice) => sum.plus(voice)).times(1.5);

    // A dark stereo echo, a different time on each side.
    const echo = feedback();
    const wet = lowpass(delay(dry.plus(echo.times(0.45)), mix(230, 310, channel)), 2800);
    echo.set(wet);
    return dry.plus(wet.times(space)).times(volume);
  },

  tick: ({ state, memory, dt, send }) => {
    const gravity = state.gravity ?? DEFAULTS.gravity;
    const bounce = state.bounce ?? DEFAULTS.bounce;
    memory.time += dt;
    memory.shake = memory.shake.map((amount) => amount * Math.exp(-4 * dt));

    // Small steps, so a fast ball cannot pass through a string between two ticks.
    const steps = Math.max(1, Math.ceil(dt / 0.005));
    const slice = dt / steps;
    for (let step = 0; step < steps; step++) {
      for (const ball of memory.balls) {
        const before = ball.y;
        ball.vy += gravity * slice;
        ball.x += ball.vx * slice;
        ball.y += ball.vy * slice;
        if (ball.x < RADIUS || ball.x > WIDTH - RADIUS) {
          ball.x = Math.min(Math.max(ball.x, RADIUS), WIDTH - RADIUS);
          ball.vx = -ball.vx * 0.8;
        }
        if (ball.vy <= 0) continue;
        strings.forEach((string, i) => {
          const top = string.y - RADIUS;
          if (before > top || ball.y < top || ball.x < string.x0 || ball.x > string.x1) return;
          ball.y = top;
          const speed = ball.vy;
          // A slow ball does not pluck: it rolls towards the nearer end and falls off.
          const centre = (string.x0 + string.x1) / 2;
          if (speed < 90) {
            ball.vy = 0;
            ball.vx += Math.sign(ball.x - centre || 1) * 600 * slice;
            return;
          }
          ball.vy = -speed * bounce;
          // A hit off centre throws the ball outwards, so it finds the other strings.
          ball.vx += ((ball.x - centre) / (string.x1 - string.x0)) * 120 + (Math.random() - 0.5) * 40;
          ball.color = string.color;
          memory.shake[i] = Math.min(10, memory.shake[i] + speed / 80);
          send(plucks[i]);
        });
      }
    }
    memory.balls = memory.balls.filter((ball) => ball.y < HEIGHT + 20 && (ball.age += dt) < 60);
  },

  view: ({ memory }) => {
    // Made again whole each tick, as the balls fall.
    const shapes = () => {
      const shapes: ShapeLiteral[] = [];
      strings.forEach((string, i) => {
        // A plucked string wobbles, its middle the most, and calms down as it rings out.
        const segments = 24;
        const wobble = memory.shake[i] * Math.sin(memory.time * (40 + i * 9));
        let last: [number, number] = [string.x0, string.y];
        for (let k = 1; k <= segments; k++) {
          const x = string.x0 + ((string.x1 - string.x0) * k) / segments;
          const point: [number, number] = [x, string.y + wobble * Math.sin((Math.PI * k) / segments)];
          shapes.push({ kind: "line", from: last, to: point, color: string.color, width: 2 });
          last = point;
        }
        shapes.push({ kind: "circle", x: string.x0, y: string.y, radius: 3, color: "text-muted" });
        shapes.push({ kind: "circle", x: string.x1, y: string.y, radius: 3, color: "text-muted" });
      });
      for (const ball of memory.balls) {
        shapes.push({ kind: "circle", x: ball.x, y: ball.y, radius: RADIUS, color: ball.color });
      }
      return shapes;
    };

    const options = (label: string, field: "root" | "scale" | "octave", values: readonly (string | number)[]) => (
      <div style={{ direction: "row", gap: "sm", align: "center" }}>
        <Text variant="label">{label}</Text>
        <SegmentedControl field={field} options={values.map((value) => ({ value, label: String(value) }))} />
      </div>
    );

    return (
      <div style={page}>
        <div style={row}>
          <Text variant="title">Gravity harp</Text>
          <Button label="Clear" variant="subtle" onClick={() => (memory.balls = [])} />
        </div>
        <Space
          width={WIDTH}
          height={HEIGHT}
          background="well"
          radius="md"
          onPress={({ x, y }) => memory.balls.push({ x, y, vx: (Math.random() - 0.5) * 60, vy: 0, color: "text", age: 0 })}
        >
          {shapes}
        </Space>
        <div style={row}>
          {options("Root", "root", ["C", "D", "E", "F", "G", "A"])}
          {options("Scale", "scale", ["major", "minor"])}
          {options("Octave", "octave", [3, 4, 5])}
        </div>
        <div style={row}>
          <Knob field="decay" />
          <Knob field="brightness" />
          <Knob field="space" />
          <Knob field="volume" />
          <Knob field="gravity" />
          <Knob field="bounce" />
        </div>
      </div>
    );
  },
});
