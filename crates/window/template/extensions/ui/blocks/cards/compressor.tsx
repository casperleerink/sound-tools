import { h, live, type Card } from "../../../sdk";
import { Cell } from "../../cell";
import { Display } from "../../display";
import { cardBinder } from "../../field";
import { Handle, type HandleAxis } from "../../handle";
import { Knob } from "../../knob";
import { Select } from "../../select";
import { DeviceCard } from "../device-card";
import { curveArea, DynamicsCurve, partOf } from "../dynamics";

/** The levels in and out, in dBFS: the range of the threshold. */
const LEVELS: [number, number] = [-60, 0];

/**
 * The compressor: what it does to each level, the threshold's handle on the curve (sideways), the
 * ratio's at its right end (up), the level it hears and how far it turns down; then threshold,
 * ratio, attack and release; behind More the knee, makeup, mix and lookahead.
 */
export const card: Card<"compressor"> = (self, ctx) => {
  const field = cardBinder<"compressor">(self, ctx);
  const { w, h: tall } = curveArea();
  const [threshold, ratio] = [field.number("threshold_db", "Threshold"), field.number("ratio", "Ratio")];
  // Above the threshold the curve rises by one over the ratio: the ratio's handle at 0 dBFS moves up by that.
  const curved = self.state.threshold_db < -1 && !ratio.held;
  const slope: HandleAxis = {
    id: "slope",
    label: ratio.label,
    value: 1 / ratio.value,
    min: 1 / ratio.max,
    max: 1,
    travel: { min: 1 + -LEVELS[0] / self.state.threshold_db, max: 1 },
    onChange: (slope) => ratio.onChange(Math.round(100 / slope) / 100),
  };
  const handles = [
    <Handle id="threshold-point" label={`${field.title} threshold point`} w={w} h={tall} x={threshold} y={partOf(live.feed("threshold_level"), LEVELS)} />,
    <Handle id="ratio-point" label={`${field.title} ratio point`} w={w} h={tall} chosen={false} x={1} y={curved ? slope : partOf(live.feed("transfer.60"), LEVELS)} />,
  ];
  const lookahead = field.choice("lookahead_ms", "Lookahead");
  return (
    <DeviceCard id="card" title={field.title} display={<Display><DynamicsCurve levels={LEVELS} handles={handles} /></Display>}
      cells={[<Knob {...threshold} />, <Knob {...ratio} />, <Knob {...field.number("attack_ms", "Attack")} />, <Knob {...field.number("release_ms", "Release")} />]}
      more={[
        <Knob {...field.number("knee_db", "Knee")} />,
        <Knob {...field.number("mix", "Mix")} unit="percent" />,
        <Knob {...field.number("makeup_db", "Makeup")} />,
        <Cell text="Lookahead"><Select {...lookahead} options={lookahead.options.map((option) => ({ ...option, text: `${option.value} ms` }))} w={56} menuW={96} /></Cell>,
      ]}
      expanded={ctx.memory.expanded === true} onExpand={() => (ctx.memory.expanded = ctx.memory.expanded !== true)} />
  );
};
