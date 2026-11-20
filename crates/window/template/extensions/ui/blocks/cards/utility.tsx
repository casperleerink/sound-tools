import { h, live, type Card } from "../../../sdk";
import theme from "../../../theme.json";
import { Cell } from "../../cell";
import { Choice } from "../../choice";
import { Display, drawingOf } from "../../display";
import { binder } from "../../field";
import { Handle } from "../../handle";
import { Knob } from "../../knob";
import { Toggle } from "../../toggle";
import { liveTravelled, stretched, travelled } from "../../value";
import { DeviceCard } from "../device-card";

/** The widest place across, as a turn of the pan: a sound twice as wide as the speakers. */
const WIDEST = Math.atan(2) / (Math.PI / 4);
/** The gain up the drawing, its whole range from `FLOOR` to `TOP`. */
const [FLOOR, TOP] = [0.08, 0.72];
const across = (pan: number) => 0.5 + pan / (2 * WIDEST);

/**
 * The utility: where its sound is heard, from its leftmost to its rightmost place, as tall as its
 * gain, with one handle (pan sideways, gain up), its channels above; then gain, pan, width and
 * mute; behind More bass mono, the frequency below which it is mono, and the inverts.
 */
export const card: Card<"utility", { expanded: boolean }> = (self, ctx) => {
  const field = binder(self, ctx);
  const { w, h: tall } = drawingOf(4, true);
  const [gain, pan] = [field.number("gain_db", "Gain"), field.number("pan", "Pan")];
  const travel = stretched(gain, FLOOR, TOP - FLOOR);
  const placed = (end: 0 | 1) => live.feed(`image.${end}`).over(2 * WIDEST).plus(0.5).times(w);
  const [left, right] = [placed(0), placed(1)];
  const height = liveTravelled(live.field("gain_db"), travel).times(tall);
  const channels = field.choice("channels", "Channels");
  // Left to right, as they sound.
  const order = ["left", "stereo", "right", "swap"];
  const display = (
    <Display strip={<Choice {...channels} options={channels.options.toSorted((a, b) => order.indexOf(String(a.value)) - order.indexOf(String(b.value)))} />} across={[[-1, "L"], [0, "C"], [1, "R"]].map(([pan, text]) => ({ at: across(pan as number), text: text as string }))} zero={travelled(0, travel)}>
      <box at={[left, height.negate().plus(tall)]} w={right.minus(left).max(1.5)} h={height} fill="text/5" border="text" borderSides={[1.5, 1.5, 0, 1.5]} opacity={self.state.mute ? 0 : 1} />
      <Handle id="gain-pan" label={`${field.title} gain and pan`} w={w} h={tall} dimmed={self.state.mute} x={{ ...pan, travel: { min: -WIDEST, max: WIDEST } }} y={{ ...gain, travel }}
        onChange={(x, y) => field.change("Change gain and pan", { pan: x, gain_db: y })} />
    </Display>
  );
  const below = (
    <col w={2 * theme.size.cell} gap={1}>
      <Cell span={2}><Toggle {...field.toggle("bass_mono", "Bass mono")} /></Cell>
      <row>
        <Knob {...field.number("bass_mono_hz", "Below")} />
        <Cell text="Invert"><row gap={1}><Toggle {...field.toggle("invert_left", "Invert left")} text="L" /><Toggle {...field.toggle("invert_right", "Invert right")} text="R" /></row></Cell>
      </row>
    </col>
  );
  return (
    <DeviceCard id="card" title={field.title} display={display}
      cells={[
        <Knob {...gain} bipolar />,
        <Knob {...pan} unit="pan" bipolar />,
        <Knob {...field.number("width", "Width")} unit="percent" bipolar />,
        <Cell><Toggle {...field.toggle("mute", "Mute")} color="peach" /></Cell>,
      ]}
      more={[below]}
      expanded={(ctx.memory.expanded ?? false)} onExpand={() => (ctx.memory.expanded = !ctx.memory.expanded)} />
  );
};
