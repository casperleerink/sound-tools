import { h, live, type Card } from "../../../sdk";
import { Cell } from "../../cell";
import { Display, drawingOf } from "../../display";
import { binder } from "../../field";
import { Handle } from "../../handle";
import { Knob } from "../../knob";
import { Toggle } from "../../toggle";
import { liveTravelled, shown, stretched, travelled } from "../../value";
import { DeviceCard } from "../device-card";

/**
 * The tail across the drawing: from `LEFT` the pre-delay's turn of `PRE`, then at least
 * `SHORTEST` and the decay's turn of `DECAY` long, falling from `TOP` to `FLOOR`. The early
 * reflections spread over `EARLY` of the drawing at the most size, every second one a mark
 * `MARK` as tall as the tail where it comes.
 */
const [LEFT, PRE, SHORTEST, DECAY, TOP, FLOOR, EARLY, MARK] = [0.04, 0.14, 0.04, 0.74, 0.88, 0.08, 0.3, 0.85];
const REFLECTIONS = 16;
/** A dashed line from the top left of its box to `[1, end]` in its units, `dashes` dashes. */
const dashed = (end: number, dashes = 12) =>
  Array.from({ length: dashes }, (_, at) => [at / dashes, (at + 0.5) / dashes].flatMap((x) => [x, 1 + (end - 1) * x])).flat();

/**
 * The reverb: its tail as it falls over a quiet fill, the highs falling sooner as a dashed line, the early
 * reflections as marks, a handle at its start (the pre-delay) and its end (the decay); then size,
 * damping, width and mix; behind More the cuts, diffusion, freeze, pre-delay and decay.
 */
export const card: Card<"reverb", { expanded: boolean }> = (self, ctx) => {
  const field = binder(self, ctx);
  const expanded = ctx.memory.expanded ?? false;
  const { w, h: tall } = drawingOf(4);
  const [preDelay, decay] = [field.number("pre_delay_ms", "Pre-delay"), field.number("decay_seconds", "Decay")];
  // Where the tail starts and ends as it plays, and where it starts in the record, which the decay's travel is laid from.
  const start = liveTravelled(live.field("pre_delay_ms"), preDelay).times(PRE).plus(LEFT);
  const length = liveTravelled(live.field("decay_seconds"), decay).times(DECAY).plus(SHORTEST);
  const started = LEFT + PRE * travelled(preDelay.value, preDelay);
  const highs = live.feed("high_decay").over(live.field("decay_seconds"));
  const spread = live.field("size").times(0.5).plus(0.5).times(EARLY);
  const marks = Array.from({ length: REFLECTIONS / 2 }, (_, at) => {
    const after = live.feed(`reflections.${2 * at}`).times(spread);
    const height = after.over(length).times(FLOOR - TOP).plus(TOP).times(MARK);
    return <box at={[start.plus(after).times(w), height.negate().plus(1).times(tall)]} w={1} h={height.times(tall)} fill="text-muted" opacity={after.lt(length)} />;
  });
  const line = (part: typeof length, points: number[], bright: boolean) => (
    <shapes at={[start.times(w), (1 - TOP) * tall]} w={part.times(w)} h={TOP * tall} units={[[0, 1], [1, 0]]} lines={[bright ? { color: "text", width: 1.5, points, fill: "text/6" } : { color: "text-secondary", width: 1, points }]} />
  );
  const display = (
    <Display>
      {marks}
      {line(length.times(highs), dashed(FLOOR / TOP), false)}
      {line(length, [0, 1, 1, FLOOR / TOP], true)}
      <Handle id="pre-delay-point" label={`${field.title} pre-delay point`} w={w} h={tall} chosen={false} x={{ ...preDelay, travel: stretched(preDelay, LEFT, PRE) }} y={TOP}
        caption={expanded ? undefined : `Pre-delay ${shown(preDelay.value, "ms")}`} />
      {/* Laid from the start in the record, moved along while the pre-delay plays another. */}
      <box at={[start.minus(started).times(w), 0]} w={w} h={tall}>
        <Handle id="decay-point" label={`${field.title} decay point`} w={w} h={tall} x={{ ...decay, travel: stretched(decay, started + SHORTEST, DECAY) }} y={FLOOR}
          caption={expanded ? undefined : `Decay ${shown(decay.value, "seconds")}`} />
      </box>
    </Display>
  );
  return (
    <DeviceCard id="card" title={field.title} display={display}
      cells={[
        <Knob {...field.number("size", "Size")} unit="percent" />,
        <Knob {...field.number("damping", "Damping")} unit="percent" />,
        <Knob {...field.number("width", "Width")} unit="percent" />,
        <Knob {...field.number("mix", "Mix")} unit="percent" />,
      ]}
      more={[
        <Knob {...field.number("low_cut_hz", "Low cut")} />,
        <Knob {...field.number("high_cut_hz", "High cut")} />,
        <Cell><Toggle {...field.toggle("freeze", "Freeze")} /></Cell>,
        <Knob {...field.number("diffusion", "Diffusion")} unit="percent" />,
        <Knob {...preDelay} />,
        <Knob {...decay} unit="seconds" />,
      ]}
      expanded={expanded} onExpand={() => (ctx.memory.expanded = !expanded)} />
  );
};
