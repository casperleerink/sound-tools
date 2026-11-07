import {
  clamp, delay, feedback, h, hsv, Knob, lowpass, bandpass, max, mix, mtof, noise, now, number, output, phasor, TAU,
  Picture, pow, saturate, saw, sine, sin, smooth, Space, Stage, tanh, tool, wrap, abs,
  type Pixel, type Signal, type Style,
} from "./sdk";

// ---------- the drone ----------

/** A slow wave from 0 to 1, `rate` times a second, starting `offset` round its cycle. */
const slow = (rate: Signal, offset: number) => sin(phasor(rate).times(TAU).plus(offset)).times(0.5).plus(0.5);

// ---------- the picture ----------

const flowingColours = ({ x, y, aspect, time, noise: field, live, last }: Pixel) => {
  const level = live(now.value("level").smooth(120));
  const react = live(now.value("react"));
  const pulse = clamp(level.times(react).times(3), 0, 1.5);
  const flow = live(now.value("flow"));
  const hueBase = live(now.value("colour"));
  const keep = live(now.value("trails"));

  const t = time.times(flow);
  const across = x.minus(0.5).times(aspect);
  const down = y.minus(0.5);
  // Warped noise: the field bends itself, so colours run like ink in water.
  const warpX = field(across.times(1.4), down.times(1.4), t);
  const warpY = field(across.times(1.4).plus(5.2), down.times(1.4).plus(1.3), t.plus(9));
  const swirl = field(across.times(2.2).plus(warpX.times(1.6)), down.times(2.2).plus(warpY.times(1.6)), t.times(0.6));

  // Ribbons of light that thicken and brighten as the drone swells.
  const ribbons = sin(swirl.times(7).plus(t.times(1.5))).times(0.5).plus(0.5);
  const glow = pow(ribbons, mix(7, 1.8, clamp(pulse, 0, 1))).times(pulse.times(1.1).plus(0.08));
  const hue = wrap(hueBase.plus(swirl.times(0.22)).plus(warpX.times(0.08)).plus(time.times(0.004)));
  const colour = hsv(hue, mix(0.85, 0.6, clamp(pulse, 0, 1)), clamp(glow, 0, 1));

  // Trails: the last frame, drifting along the flow and breathing outwards with the level.
  const zoom = mix(0.998, 0.985, clamp(pulse, 0, 1));
  const fromX = mix(0.5, x, zoom).plus(warpY.times(0.0025));
  const fromY = mix(0.5, y, zoom).minus(warpX.times(0.0025));
  const trail = last(fromX, fromY);
  return {
    red: max(colour.red, trail.red.times(keep)),
    green: max(colour.green, trail.green.times(keep)),
    blue: max(colour.blue, trail.blue.times(keep)),
  };
};

const panel: Style = { gap: 16, padding: 24, grow: true, background: "gray-50" };
const row: Style = { direction: "row", gap: 4, align: "center", wrap: true };
const heading: Style = { fontSize: 11, weight: "semibold", color: "gray-600" };

tool({
  name: "drone-light",
  title: "Drone and light",
  when: "You change the drone or the projected colours of this piece",
  doc:
    "A slow drone that never repeats, and flowing colours for a projector that pulse with how loud the drone is and leave trails. " +
    "The drone is a root, its fifth, its octave and a sub an octave down, each swelling on its own slow cycle, through a soft filter and a wash of echoes. " +
    "`root` is the note it sits on. `brightness` opens the filter: low is dark and warm, high is reedy. `motion` is how fast the layers and the filter drift: low is glacial, high breathes every few seconds. " +
    "`swell` is how far the loudness rises and falls. `fifth` and `octave` bring in those layers, `sub` the low one. `beating` detunes left and right against each other, slow shimmer to sour. `air` adds breath. `wash` is how much of the echoing space. `volume` is the whole level. " +
    "For the colours: `react` is how strongly they follow the loudness, `trails` how long the light lingers, `flow` how fast the colours move, `colour` the hue they start from.",
  state: {
    root: number({ min: 24, max: 60, default: 38, integer: true, label: "Root" }),
    brightness: number({ min: 80, max: 5000, default: 900, unit: "hz", label: "Brightness" }),
    motion: number({ min: 0.005, max: 0.3, default: 0.04, unit: "hz", label: "Motion" }),
    swell: number({ min: 0, max: 1, default: 0.6, unit: "percent", label: "Swell" }),
    fifth: number({ min: 0, max: 1, default: 0.5, unit: "percent", label: "Fifth" }),
    octave: number({ min: 0, max: 1, default: 0.35, unit: "percent", label: "Octave" }),
    sub: number({ min: 0, max: 1, default: 0.35, unit: "percent", label: "Sub" }),
    beating: number({ min: 0, max: 30, default: 6, label: "Beating" }),
    air: number({ min: 0, max: 1, default: 0.2, unit: "percent", label: "Air" }),
    wash: number({ min: 0, max: 0.9, default: 0.55, unit: "percent", label: "Wash" }),
    volume: number({ min: 0, max: 1, default: 0.7, unit: "percent", label: "Volume" }),
    react: number({ min: 0, max: 4, default: 1.5, label: "React" }),
    trails: number({ min: 0.8, max: 0.995, default: 0.96, label: "Trails" }),
    flow: number({ min: 0, max: 0.5, default: 0.08, label: "Flow" }),
    colour: number({ min: 0, max: 1, default: 0.6, label: "Colour" }),
  },
  sound: ({ root, brightness, motion, swell, fifth, octave, sub, beating, air, wash, volume }) => {
    const pitch = smooth(root, 2000); // a new root slides over two seconds
    const cents = (side: number) => beating.times(side).over(1200);

    const side = (sign: number, seed: number) => {
      // Each layer breathes on its own cycle, rates in odd ratios so the blend never repeats.
      const layer = (interval: number, amount: Signal | number, rate: number, offset: number) => {
        const hz = mtof(pitch.plus(interval).plus(cents(sign).times(12)));
        const tone = saw(hz).plus(saw(hz.times(1.003)).times(0.6));
        const breathe = slow(motion.times(rate), offset + seed);
        return tone.times(mix(0.35, 1, breathe)).times(amount);
      };
      const body = layer(0, 1, 1, 0)
        .plus(layer(7, fifth, 1.37, 1.7))
        .plus(layer(12, octave.times(0.8), 0.71, 3.1))
        .plus(layer(19, octave.times(0.3), 1.93, 4.4));
      const cutoff = brightness.times(mix(0.5, 1.6, slow(motion.times(0.53), 2.2 + seed)));
      const filtered = lowpass(lowpass(body, cutoff, 0.9), cutoff.times(1.5));
      const low = sine(mtof(pitch.minus(12))).times(sub).times(0.9);
      const breath = bandpass(noise(), mtof(pitch.plus(36)).times(mix(0.8, 1.6, slow(motion.times(0.8), seed))), 3)
        .times(air).times(2);
      const swellAmount = mix(1, slow(motion.times(0.29), 0.9), swell);
      return filtered.times(0.4).plus(low).plus(breath).times(swellAmount);
    };

    const dryL = side(-1, 0);
    const dryR = side(1, 0.6);

    // A wash of echoes that cross from side to side and darken as they go.
    const backL = feedback();
    const backR = feedback();
    const wetL = delay(dryL.plus(backR.times(wash)), 613);
    const wetR = delay(dryR.plus(backL.times(wash)), 857);
    backL.set(lowpass(delay(wetL, 211), 1800));
    backR.set(lowpass(delay(wetR, 293), 1800));

    const left = tanh(dryL.plus(wetL.times(wash).times(0.8)).times(volume));
    const right = tanh(dryR.plus(wetR.times(wash).times(0.8)).times(volume));

    // How loud the drone is, for the colours.
    output("level", smooth(abs(left).plus(abs(right)).times(0.5), 80));
    return { left, right };
  },
  view: ({ size }) => (
    <div style={panel}>
      <Stage>
        <Space key="stage" grow>
          <Picture x={0} y={0} width={now.space("stage").right} height={now.space("stage").bottom} across={360}
            pixels={flowingColours} />
        </Space>
      </Stage>
      <div style={{ direction: "row", gap: 24, grow: true }}>
        <div style={{ gap: 8 }}>
          <div style={heading}>On the projector</div>
          <Space width={() => Math.max(240, Math.min(560, size.width * 0.45))} height={() => Math.max(135, Math.min(315, size.width * 0.45 * 0.5625))}
            background="gray-950" radius={6}>
            <Picture x={0} y={0} width={() => Math.max(240, Math.min(560, size.width * 0.45))}
              height={() => Math.max(135, Math.min(315, size.width * 0.45 * 0.5625))} across={200} pixels={flowingColours} />
          </Space>
        </div>
        <div style={{ gap: 16, grow: true }}>
          <div style={heading}>Drone</div>
          <div style={row}>
            <Knob field="root" label="Root" min={24} max={60} default={38} />
            <Knob field="brightness" label="Brightness" min={80} max={5000} default={900} unit="hz" />
            <Knob field="motion" label="Motion" min={0.005} max={0.3} default={0.04} unit="hz" />
            <Knob field="swell" label="Swell" min={0} max={1} default={0.6} unit="percent" />
            <Knob field="beating" label="Beating" min={0} max={30} default={6} />
            <Knob field="wash" label="Wash" min={0} max={0.9} default={0.55} unit="percent" />
          </div>
          <div style={row}>
            <Knob field="fifth" label="Fifth" min={0} max={1} default={0.5} unit="percent" />
            <Knob field="octave" label="Octave" min={0} max={1} default={0.35} unit="percent" />
            <Knob field="sub" label="Sub" min={0} max={1} default={0.35} unit="percent" />
            <Knob field="air" label="Air" min={0} max={1} default={0.2} unit="percent" />
            <Knob field="volume" label="Volume" min={0} max={1} default={0.7} unit="percent" />
          </div>
          <div style={heading}>Light</div>
          <div style={row}>
            <Knob field="react" label="React" min={0} max={4} default={1.5} />
            <Knob field="trails" label="Trails" min={0.8} max={0.995} default={0.96} />
            <Knob field="flow" label="Flow" min={0} max={0.5} default={0.08} />
            <Knob field="colour" label="Colour" min={0} max={1} default={0.6} />
          </div>
        </div>
      </div>
    </div>
  ),
});
