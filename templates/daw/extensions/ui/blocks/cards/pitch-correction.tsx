import { h, live, type Card } from "../../../sdk";
import { Cell } from "../../cell";
import { Display, drawingOf } from "../../display";
import { binder } from "../../field";
import { Knob } from "../../knob";
import { Select } from "../../select";
import { Toggle } from "../../toggle";
import { DeviceCard } from "../device-card";

/** The heights of the line of what is sung and of the block of how far it is moved. */
const [SUNG, MOVED] = [0.8, 0.55];

/**
 * Pitch correction: around the note it moves the voice to, ±50 cents across, a line where the
 * voice is sung and a soft block of how far it is moved, its key and scale above; then retune
 * and humanize; behind More the voice, ignore vibrato, formants and mix.
 */
export const card: Card<"pitch-correction", { expanded: boolean }> = (self, ctx) => {
  const field = binder(self, ctx);
  const { w, h: tall } = drawingOf(4, true);
  const [heard, target, correction] = [live.feed("heard"), live.feed("target"), live.feed("correction")];
  // Semitones from the target are halves of the drawing: it is ±50 cents wide.
  const [sung, plays] = [heard.minus(target).plus(0.5).clamp(0, 1), heard.plus(correction).minus(target).plus(0.5).clamp(0, 1)];
  const moving = heard.gt(0).times(target.gt(0));
  const strip = [<Select {...field.choice("key", "Key")} w={56} menuW={96} />, <Select {...field.choice("scale", "Scale")} w={140} menuW={160} />];
  const display = (
    <Display strip={strip} across={[{ at: 0.25, text: "−25" }, { at: 0.5, text: "0" }, { at: 0.75, text: "+25 ct" }]}>
      <box at={[sung.min(plays).times(w), (1 - MOVED) * tall]} w={plays.minus(sung).abs().times(w)} h={MOVED * tall} fill="text/16" opacity={moving} />
      <box at={[sung.times(w).minus(0.75), (1 - SUNG) * tall]} w={1.5} h={SUNG * tall} fill="text" opacity={moving} />
      <row at={[6, tall - 22]} h={18} pad={[1, 0]} gap={1} align="center" radius="sm" fill="well" opacity={heard.gt(0)}>
        <text style="number" color="text-secondary" live={heard} unit="pitch" />
        <row gap={1} opacity={target.gt(0)}>
          <text style="number" color="text-secondary">→</text>
          <text style="number" color="text-secondary" live={target} unit="pitch" />
          <text style="number" color="text-secondary">·</text>
          <text style="number" color="text-secondary" live={correction.times(100).round()} unit="cents" />
        </row>
      </row>
    </Display>
  );
  return (
    <DeviceCard id="card" title={field.title} display={display}
      cells={[<Knob {...field.number("retune_ms", "Retune")} />, <Knob {...field.number("humanize", "Humanize")} unit="percent" />]}
      more={[
        <Cell text="Voice" span={2}><Select {...field.choice("voice", "Voice")} w={120} menuW={160} /></Cell>,
        <Cell span={2}><Toggle {...field.toggle("keep_formants", "Formants")} /></Cell>,
        <Cell span={2}><Toggle {...field.toggle("ignore_vibrato", "Ignore vibrato")} /></Cell>,
        <Knob {...field.number("mix", "Mix")} unit="percent" />,
      ]}
      expanded={(ctx.memory.expanded ?? false)} onExpand={() => (ctx.memory.expanded = !ctx.memory.expanded)} />
  );
};
