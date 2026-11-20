import { h, live, type Card } from "../../../sdk";
import { Choice } from "../../choice";
import { Display, drawingOf } from "../../display";
import { binder } from "../../field";
import { Handle } from "../../handle";
import { Knob } from "../../knob";
import { DeviceCard } from "../device-card";

/**
 * Two cycles of the LFO across, about the `MIDDLE`, swinging `SWING` up and down at full depth;
 * the depth's handle on the peak at `DEPTH_AT`. The second line lags by half a cycle at full
 * spread: its handle on its first peak.
 */
const [MIDDLE, SWING, DEPTH_AT, POINTS] = [0.5, 0.36, 0.625, 96];
const DEPTH_TRAVEL = { min: -MIDDLE / SWING, max: (1 - MIDDLE) / SWING };
const SPREAD_TRAVEL = { min: -0.5, max: 3.5 };
/** Cycles of a sine, `cycles` of them over 0 to `cycles / 2` across and -1 to 1 up, as segments; every other one left out for a dashed line. */
const sine = (cycles: number, dashed: boolean) =>
  Array.from({ length: POINTS * cycles }, (_, at) => [at, at + 1].flatMap((step) => [step / POINTS / 2, Math.sin((2 * Math.PI * step) / POINTS)]))
    .filter((_, at) => !dashed || at % 4 < 2)
    .flat();
const [LINE, LAGGED] = [sine(2, false), sine(3, true)];

/**
 * Modulation: two cycles of its LFO, the second line lagging by the spread, a handle on the
 * peak of each (depth up, spread sideways), the mode above; then rate, depth, feedback and mix;
 * behind More the spread.
 */
export const card: Card<"modulation", { expanded: boolean }> = (self, ctx) => {
  const field = binder(self, ctx);
  const expanded = ctx.memory.expanded ?? false;
  const { w, h: tall } = drawingOf(4, true);
  const [depth, spread] = [field.number("depth", "Depth"), field.number("spread", "Spread")];
  const swing = live.field("depth").times(SWING);
  // A cycle is half the drawing: the second line starts a cycle left of it, moved right by the lag.
  const lag = live.field("spread").over(4);
  const display = (
    <Display strip={<Choice {...field.choice("mode", "Mode")} />} zero={MIDDLE}>
      <shapes at={[lag.minus(0.5).times(w), swing.negate().plus(1 - MIDDLE).times(tall)]} w={1.5 * w} h={swing.times(2 * tall)} units={[[0, 1.5], [1, -1]]} lines={[{ color: "text-secondary", width: 1, points: LAGGED }]} />
      <shapes at={[0, swing.negate().plus(1 - MIDDLE).times(tall)]} w={w} h={swing.times(2 * tall)} units={[[0, 1], [1, -1]]} lines={[{ color: "text", width: 1.5, points: LINE }]} />
      <Handle id="spread-point" label={`${field.title} spread point`} w={w} h={tall} chosen={false} x={{ ...spread, travel: SPREAD_TRAVEL }} y={swing.plus(MIDDLE)}
        caption={expanded ? undefined : `Spread ${Math.round(spread.value * 180)}°`} />
      <Handle id="depth-point" label={`${field.title} depth point`} w={w} h={tall} x={DEPTH_AT} y={{ ...depth, travel: DEPTH_TRAVEL }} />
    </Display>
  );
  return (
    <DeviceCard id="card" title={field.title} display={display}
      cells={[
        <Knob {...field.number("rate_hz", "Rate")} />,
        <Knob {...depth} unit="percent" />,
        <Knob {...field.number("feedback", "Feedback")} unit="percent" />,
        <Knob {...field.number("mix", "Mix")} unit="percent" />,
      ]}
      more={[<Knob {...spread} said={`${Math.round(spread.value * 180)}°`} />]}
      expanded={expanded} onExpand={() => (ctx.memory.expanded = !expanded)} />
  );
};
