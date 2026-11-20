import { h, live, type Card } from "../../../sdk";
import { Cell } from "../../cell";
import { Display } from "../../display";
import { binder } from "../../field";
import { Handle } from "../../handle";
import { Knob } from "../../knob";
import { Select } from "../../select";
import { DeviceCard } from "../device-card";
import { curveArea, partOf, TurnedDown } from "../dynamics";

/** The levels up the drawing, in dBFS: the range of the ceiling. */
const LEVELS: [number, number] = [-24, 0];
const INSET = 6;

/**
 * The limiter: what it sent out over the last 4 s under the line of its ceiling, whose handle at
 * the right drags it up and down, and how far it turns down now; then gain, ceiling, release and
 * lookahead.
 */
export const card: Card<"limiter"> = (self, ctx) => {
  const field = binder(self, ctx);
  const { w, h: tall } = curveArea();
  // No lane moves the ceiling, so it is no number path of the limiter's.
  const ceiling = field.number("ceiling_db" as "gain_db", "Ceiling");
  const lookahead = field.choice("lookahead_ms", "Lookahead");
  const display = (
    <Display>
      <box at={[0, INSET]} w={w} h={tall}>
        <shapes at={[INSET, 0]} w={w - INSET} h={tall} units={[[0, 1], [LEVELS[1], LEVELS[0]]]} lines={[{ color: "green", width: 1.5, points: live.feed("history"), fill: "green/16" }]} />
        <box at={[0, partOf(live.field("ceiling_db"), LEVELS).negate().plus(1).times(tall)]} w={w} h={1} fill="text" />
        <Handle id="ceiling-point" label={`${field.title} ceiling point`} w={w} h={tall} x={1} y={ceiling} />
      </box>
      <TurnedDown />
    </Display>
  );
  return (
    <DeviceCard id="card" title={field.title} display={display}
      cells={[
        <Knob {...field.number("gain_db", "Gain")} />,
        <Knob {...ceiling} />,
        <Knob {...field.number("release_ms", "Release")} />,
        <Cell text="Lookahead"><Select {...lookahead} options={lookahead.options.map((option) => ({ ...option, text: `${option.value} ms` }))} w={62} menuW={96} /></Cell>,
      ]} />
  );
};
