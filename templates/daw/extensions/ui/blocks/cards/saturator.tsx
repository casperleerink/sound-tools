import { h, live, type Card } from "../../../sdk";
import { Choice } from "../../choice";
import { Curve } from "../../curve";
import { Display, drawingOf, heightOf } from "../../display";
import { binder } from "../../field";
import { Handle } from "../../handle";
import { Knob } from "../../knob";
import { DeviceCard } from "../device-card";

/** Levels in dBFS, the input across and the output up: under the bend at the most drive, and room over full scale. */
const LEVELS: [number, number] = [6, -48];
/** The level a sine keeps at every drive, which the automatic gain holds to. */
const REFERENCE = heightOf(-12, LEVELS);

/**
 * The saturator: what comes out for every level that goes in, the straight line of the sound as
 * it came in, a handle at the bend that drags the drive (left is more), its curve above; then
 * drive, tone, output and mix.
 */
export const card: Card<"saturator"> = (self, ctx) => {
  const field = binder(self, ctx);
  const { w, h: tall } = drawingOf(4, true);
  const drive = field.number("drive_db", "Drive");
  // The bend is at -drive dBFS in: more drive is further left.
  const bend = { ...drive, travel: { min: -LEVELS[1], max: -LEVELS[0] } };
  const display = (
    <Display strip={<Choice {...field.choice("curve", "Curve")} />} across={[{ at: REFERENCE }, { at: heightOf(0, LEVELS) }]} up={[{ at: REFERENCE }]} zero={heightOf(0, LEVELS)}>
      <shapes at={[0, 0]} w={w} h={tall} lines={[{ color: "text-muted", width: 1, points: [0, tall, w, 0] }]} />
      <Curve heights={live.feed("transfer")} w={w} h={tall} across={[LEVELS[1], LEVELS[0]]} range={LEVELS} />
      <Handle id="drive" label={`${field.title} bend`} w={w} h={tall} x={bend} y={live.feed("bend").minus(LEVELS[1]).over(LEVELS[0] - LEVELS[1])}
        caption={<row gap={1}><text style="number" color="text-secondary">Auto gain</text><text style="number" color="text-secondary" live={live.feed("auto_gain")} unit="db" /></row>} />
    </Display>
  );
  return (
    <DeviceCard id="card" title={field.title} display={display}
      cells={[<Knob {...drive} />, <Knob {...field.number("tone_db", "Tone")} bipolar />, <Knob {...field.number("output_db", "Output")} bipolar />, <Knob {...field.number("mix", "Mix")} unit="percent" />]} />
  );
};
