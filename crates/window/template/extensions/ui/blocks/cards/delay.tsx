import { h, live, Live, type Card } from "../../../sdk";
import theme from "../../../theme.json";
import { Cell } from "../../cell";
import { Display, drawingOf } from "../../display";
import { cardBinder } from "../../field";
import { Handle, type HandleAxis } from "../../handle";
import { Knob } from "../../knob";
import { Segmented } from "../../segmented";
import { Select } from "../../select";
import { Toggle } from "../../toggle";
import { liveTravelled, stretched, travelled } from "../../value";
import { DeviceCard } from "../device-card";

/**
 * The repeats as narrow peaks along the drawing: the sound at `LEFT`, then one every gap, from
 * `SHORTEST` plus the time's turn of `ZONE`; as tall as `TOP` falling by the feedback, each
 * `PEAK` wide, until one is under `LOWEST` or past the right.
 */
const [LEFT, SHORTEST, ZONE, TOP, PEAK, LOWEST] = [0.04, 0.03, 0.4, 0.88, 0.012, 0.01];
/** The most repeats that fit, at the shortest gap. */
const MOST = Math.floor((1 - 2 * LEFT) / SHORTEST);

/**
 * The delay: its repeats, each quieter by the feedback, with a handle on the second (time
 * sideways, feedback up), the feel above while it syncs to the tempo; then the time (a note while
 * it syncs), sync, feedback and mix; behind More the cuts and ping-pong.
 */
export const card: Card<"delay"> = (self, ctx) => {
  const field = cardBinder<"delay">(self, ctx);
  const { w, h: tall } = drawingOf(4, true);
  const { sync } = self.state;
  const [time, feedback] = [field.number("time_ms", "Time"), field.number("feedback", "Feedback")];
  const division = field.choice("division", "Time");
  const notes = division.options.map((option) => option.value);
  const steps = { min: 0, max: notes.length - 1 };
  // While it syncs, the handle steps through the notes; else it turns the time.
  const across: HandleAxis = sync
    ? { id: "division", label: division.label, value: notes.indexOf(division.value), ...steps, step: 1, travel: stretched(steps, LEFT + 2 * SHORTEST, 2 * ZONE), onChange: () => {} }
    : { ...time, travel: stretched(time, LEFT + 2 * SHORTEST, 2 * ZONE) };
  const turned = sync ? Live.of(travelled(notes.indexOf(division.value), steps)) : liveTravelled(live.field("time_ms"), time);
  const gap = turned.times(ZONE).plus(SHORTEST);
  const level = live.field("feedback");
  const repeats = Array.from({ length: MOST }, (_, at) => {
    const [x, height] = [gap.times(at + 1).plus(LEFT), level.pow(at).times(TOP)];
    return (
      <shapes at={[x.minus(PEAK / 2).times(w), height.negate().plus(1).times(tall)]} w={PEAK * w} h={height.times(tall)} units={[[0, 1], [1, 0]]}
        opacity={x.le(1 - LEFT - PEAK / 2).times(height.ge(LOWEST * TOP))} lines={[{ color: "text", width: 1.5, points: [0, 0, 0.5, 1, 0.5, 1, 1, 0] }]} />
    );
  });
  const display = (
    <Display keepStrip strip={sync ? <Segmented {...field.choice("feel", "Feel")} /> : undefined}>
      <box at={[0, tall - 1.5]} w={w} h={1.5} fill="text" />
      <box at={[LEFT * w, (1 - TOP) * tall]} w={1} h={TOP * tall} fill="text-muted" />
      {repeats}
      <Handle id="time-feedback" label={`${field.title} time and feedback`} w={w} h={tall} x={across} y={{ ...feedback, travel: { min: 0, max: 1 / TOP } }}
        onChange={(x, y) => field.change("Change time and feedback", { ...(sync ? { division: notes[Math.round(x ?? 0)] } : { time_ms: x }), feedback: y })} />
    </Display>
  );
  const cuts = (
    <col w={2 * theme.size.cell} gap={1}>
      <row><Knob {...field.number("low_cut_hz", "Low cut")} /><Knob {...field.number("high_cut_hz", "High cut")} /></row>
      <Cell span={2}><Toggle {...field.toggle("ping_pong", "Ping-pong")} /></Cell>
    </col>
  );
  return (
    <DeviceCard id="card" title={field.title} display={display}
      cells={[
        sync ? <Cell text="Time"><Select {...division} w={56} menuW={96} /></Cell> : <Knob {...time} />,
        <Cell><Toggle {...field.toggle("sync", "Sync")} /></Cell>,
        <Knob {...feedback} unit="percent" />,
        <Knob {...field.number("mix", "Mix")} unit="percent" />,
      ]}
      more={[cuts]}
      expanded={ctx.memory.expanded === true} onExpand={() => (ctx.memory.expanded = ctx.memory.expanded !== true)} />
  );
};
