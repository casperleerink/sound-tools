import { h, live, type Card } from "../../../sdk";
import { Cell } from "../../cell";
import { Choice } from "../../choice";
import { Curve } from "../../curve";
import { Display, drawingOf, FREQUENCY_TICKS, heightOf, OCTAVES } from "../../display";
import { cardBinder } from "../../field";
import { Handle } from "../../handle";
import { Knob } from "../../knob";
import { DeviceCard } from "../device-card";

/** Room for the peak of full resonance at the top. */
const DB: [number, number] = [18, -36];
const TYPES: Record<string, string> = { low_pass: "Low", band_pass: "Band", high_pass: "High", notch: "Notch" };

/**
 * The resonance up the drawing so that the handle at the cutoff sits on the peak of the curve:
 * the peak in dB is a straight line over the resonance, from `none` to `full`.
 */
const onThePeak = ([none = 0, full = 1]: readonly number[]) => ({ min: (DB[1] - none) / (full - none), max: (DB[0] - none) / (full - none) });

/** The filter: its response with a handle at the cutoff (resonance up), its type above, then cutoff, resonance, drive and mix; behind More the slope, character and LFO. */
export const card: Card<"filter"> = (self, ctx) => {
  const field = cardBinder<"filter">(self, ctx);
  const { w, h: tall } = drawingOf(4, true);
  const type = field.choice("type", "Type");
  const [cutoff, resonance] = [field.number("cutoff_hz", "Cutoff"), field.number("resonance", "Resonance")];
  const display = (
    <Display strip={<Choice {...type} options={type.options.map((option) => ({ ...option, text: TYPES[option.value] ?? option.text }))} />} across={FREQUENCY_TICKS} zero={heightOf(0, DB)}>
      <Curve heights={live.feed("response")} w={w} h={tall} across={OCTAVES} range={DB} />
      <Handle id="cutoff-resonance" label={`${field.title} cutoff and resonance`} w={w} h={tall} x={cutoff} y={{ ...resonance, travel: onThePeak(self.facts.peak_db) }}
        onChange={(hz, amount) => field.change("Change cutoff and resonance", { cutoff_hz: hz, resonance: amount })} />
    </Display>
  );
  const slope = field.choice("slope", "Slope");
  return (
    <DeviceCard id="card" title={field.title} display={display}
      cells={[<Knob {...cutoff} />, <Knob {...resonance} unit="percent" />, <Knob {...field.number("drive_db", "Drive")} />, <Knob {...field.number("mix", "Mix")} unit="percent" />]}
      more={[
        <Cell text="Slope" span={2}><Choice {...slope} options={slope.options.map((option) => ({ ...option, text: String(option.value) }))} /></Cell>,
        <Cell span={2}><Choice {...field.choice("character", "Character")} /></Cell>,
        <Knob {...field.number("lfo_rate_hz", "LFO rate")} />,
        <Knob {...field.number("lfo_depth_octaves", "LFO depth")} unit="octaves" />,
      ]}
      expanded={ctx.memory.expanded === true} onExpand={() => (ctx.memory.expanded = ctx.memory.expanded !== true)} />
  );
};
