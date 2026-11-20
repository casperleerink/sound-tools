import { h, live, type Card } from "../../../sdk";
import theme from "../../../theme.json";
import { Cell } from "../../cell";
import { Choice } from "../../choice";
import { Curve } from "../../curve";
import { Display, drawingOf } from "../../display";
import { cardBinder } from "../../field";
import { Handle } from "../../handle";
import { Knob } from "../../knob";
import { shown, travelled, type Range } from "../../value";
import { DeviceCard } from "../device-card";

/**
 * The envelope feed lays each time out over its knob's travel, 0 to 1, one after the other, the
 * note held for `HOLD` between decay and release. The drawing shows from `LEFT` to `RIGHT` of
 * that, and levels from `BOTTOM` to `TOP`, so a handle at an edge can be taken.
 */
const [HOLD, LEFT, RIGHT] = [0.36, -0.14, 3.43];
const LEVEL: [number, number] = [1.15, -0.1];

/** The travel of a time whose stage starts at `start` across the drawing, as the feed lays it out: `time`'s own range, moved along. */
const stage = (time: Range, start: number): Range => {
  const ratio = time.max / time.min;
  const min = time.min * ratio ** (LEFT - start);
  return { min, max: min * ratio ** (RIGHT - LEFT), scale: "log" };
};

/**
 * The synth: its envelope with a handle at the end of each stage (the corner after the decay
 * moves decay and sustain), the wave above it; cutoff, resonance, character and gain; behind
 * More the four knobs of the envelope.
 */
export const card: Card<"instrument.synth"> = (self, ctx) => {
  const field = cardBinder<"instrument.synth">(self, ctx);
  const { memory } = ctx;
  const expanded = memory.expanded === true;
  const { w, h: tall } = drawingOf(4, true);
  const [attack, decay, sustain, release] = [field.number("attack_seconds", "Attack"), field.number("decay_seconds", "Decay"), field.number("sustain", "Sustain"), field.number("release_seconds", "Release")];
  const peak = travelled(attack.value, attack);
  const held = peak + travelled(decay.value, decay) + HOLD;
  const top = (1 - LEVEL[1]) / (LEVEL[0] - LEVEL[1]);
  const bottom = -LEVEL[1] / (LEVEL[0] - LEVEL[1]);
  // Expanded, the knobs say the times; else a handle says its own while pointed at.
  const say = (text: string) => (expanded ? undefined : text);
  const display = (
    <Display strip={<Choice {...field.choice("waveform", "Waveform")} />}>
      <Curve heights={live.feed("envelope")} w={w} h={tall} across={[LEFT, RIGHT]} range={LEVEL} />
      <Handle id="attack" label={`${field.title} attack point`} w={w} h={tall} x={{ ...attack, travel: stage(attack, 0) }} y={top} caption={say(`Attack ${shown(attack.value, "seconds")}`)} />
      <Handle id="decay" label={`${field.title} decay and sustain`} w={w} h={tall} x={{ ...decay, travel: stage(decay, peak) }} y={{ ...sustain, travel: { min: LEVEL[1], max: LEVEL[0] } }}
        caption={say(`Decay ${shown(decay.value, "seconds")} · Sustain ${shown(sustain.value, "percent")}`)}
        onChange={(seconds, level) => field.change("Change decay and sustain", { decay_seconds: seconds, sustain: level })} />
      <Handle id="release" label={`${field.title} release point`} w={w} h={tall} x={{ ...release, travel: stage(release, held) }} y={bottom} caption={say(`Release ${shown(release.value, "seconds")}`)} />
    </Display>
  );
  // The character is the filter's, so it sits under the filter's two knobs.
  const filter = (
    <col w={2 * theme.size.cell} gap={1}>
      <row><Knob {...field.number("cutoff_hz", "Cutoff")} /><Knob {...field.number("resonance", "Resonance")} unit="percent" /></row>
      <Cell span={2}><Choice {...field.choice("character", "Character")} /></Cell>
    </col>
  );
  return (
    <DeviceCard id="card" title={field.title} display={display}
      cells={[filter, <Knob {...field.number("gain", "Gain")} unit="percent" />]}
      more={[<Knob {...attack} unit="seconds" />, <Knob {...decay} unit="seconds" />, <Knob {...sustain} unit="percent" />, <Knob {...release} unit="seconds" />]}
      expanded={expanded} onExpand={() => (memory.expanded = !expanded)} />
  );
};
