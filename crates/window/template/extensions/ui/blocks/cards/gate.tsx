import { h, live, type Card } from "../../../sdk";
import { Display } from "../../display";
import { cardBinder } from "../../field";
import { Handle } from "../../handle";
import { Knob } from "../../knob";
import { DeviceCard } from "../device-card";
import { curveArea, DynamicsCurve, partOf } from "../dynamics";

/** The levels in and out, in dBFS: the range of the threshold. */
const LEVELS: [number, number] = [-80, 0];

/**
 * The gate: what it lets through of each level, a step down by its range under the threshold,
 * whose handle drags sideways; the level it hears and how far it turns down; then threshold and
 * range, attack and release, transient and sustain; behind More the hold.
 */
export const card: Card<"gate"> = (self, ctx) => {
  const field = cardBinder<"gate">(self, ctx);
  const { w, h: tall } = curveArea();
  const threshold = field.number("threshold_db", "Threshold");
  // Under the threshold it turns down by its range: what comes out is under the drawing up to the bottom plus the range.
  const floor = live.field("range_db").plus(LEVELS[0]).min(live.field("threshold_db"));
  const handle = <Handle id="threshold-point" label={`${field.title} threshold point`} w={w} h={tall} x={threshold} y={partOf(live.field("threshold_db"), LEVELS)} />;
  return (
    <DeviceCard id="card" title={field.title} display={<Display><DynamicsCurve levels={LEVELS} floor={floor} handles={handle} /></Display>}
      cells={[
        <Knob {...threshold} />,
        <Knob {...field.number("attack_ms", "Attack")} />,
        <Knob {...field.number("transient_db", "Transient")} bipolar />,
        <Knob {...field.number("range_db", "Range")} />,
        <Knob {...field.number("release_ms", "Release")} />,
        <Knob {...field.number("sustain_db", "Sustain")} bipolar />,
      ]}
      more={[<Knob {...field.number("hold_ms", "Hold")} />]}
      expanded={ctx.memory.expanded === true} onExpand={() => (ctx.memory.expanded = ctx.memory.expanded !== true)} />
  );
};
