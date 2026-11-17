import { h, live, type Unit } from "../sdk";
import theme from "../theme.json";
import { TypeIn } from "./type-in";
import { fitted, liveValueAt, stepped, travelled, valueAt, type Range } from "./value";

const [DIAL, CELL, ROW] = [theme.size.knob, theme.size.cell, theme.size.row];
/** Points of drag for the whole travel. */
const TRAVEL = 200;
/** The width of the arc, and from the edge of the dial to its face, as the window's own card has them. */
const [ARC, RIM, SWEEP] = [2.5, 7.5, 270];
const RADIUS = DIAL / 2 - ARC / 2;
/** The arc of the whole travel, as short segments from the bottom left round to the bottom right. */
const ARC_POINTS: number[] = [];
for (let i = 0; i < 48; i++) {
  const angle = (t: number) => ((135 + t * SWEEP) * Math.PI) / 180;
  const [a, b] = [angle(i / 48), angle((i + 1) / 48)];
  ARC_POINTS.push(DIAL / 2 + Math.cos(a) * RADIUS, DIAL / 2 + Math.sin(a) * RADIUS, DIAL / 2 + Math.cos(b) * RADIUS, DIAL / 2 + Math.sin(b) * RADIUS);
}

export type KnobProps = Range & {
  id: string;
  /** What it is called, for an agent; also shown under the dial unless `text` says it shorter. */
  label: string;
  text?: string;
  value: number;
  /** What a double click or Backspace sets. */
  initial?: number;
  unit?: Unit;
  /** Its arc starts from the middle, as a pan's. */
  bipolar?: boolean;
  onChange: (value: number) => void;
};

/**
 * A knob in a cell of a card: the dial, its name and its value. Drag up or down (with Shift,
 * ten times finer), arrows step it, a double click or Backspace sets `initial`, and a click on
 * the value or Enter types one. The arc is the one bright line and follows the drag.
 */
export const Knob = ({ id, label, text, value, initial, unit, bipolar = false, onChange, ...range }: KnobProps) => {
  const at = travelled(value, range);
  const set = (next: number) => onChange(fitted(next, range));
  // A drag from the value line turns the knob too.
  const dragged = live.drag(id).dy.plus(live.drag(`${id}-value`).dy).over(-TRAVEL).plus(at);
  const angle = dragged.times((SWEEP * Math.PI) / 180).plus((135 * Math.PI) / 180);
  const pointer = (cos: boolean) => (cos ? angle.cos() : angle.sin()).times(DIAL / 2 - RIM - 4).plus(DIAL / 2 - 1.5);
  const drag = { axis: "y", min: -(1 - at) * TRAVEL, max: at * TRAVEL } as const;
  const dragEnd = ({ dy }: { dy: number }) => onChange(valueAt(at - dy / TRAVEL, range));
  const actions = { set, increment: (by: number) => onChange(stepped(value, by, range)), decrement: (by: number) => onChange(stepped(value, -by, range)), ...(initial === undefined ? {} : { reset: () => set(initial) }) };
  const readout = liveValueAt(dragged, range);
  return (
    <col key={id} label={label} role="slider" value={value} w={CELL} h={ROW} align="center" radius="md" drag={drag} onDragEnd={dragEnd} actions={actions}>
      <box w={DIAL} h={DIAL}>
        <shapes at={[0, 0]} w={DIAL} h={DIAL} lines={[{ color: "fill-strong", width: ARC, points: ARC_POINTS }]} />
        <shapes at={[0, 0]} w={DIAL} h={DIAL} lines={[{ color: "text", width: ARC, points: ARC_POINTS }]} trim={bipolar ? [dragged.min(0.5), dragged.max(0.5)] : [0, dragged]} />
        <box at={[RIM, RIM]} w={DIAL - 2 * RIM} h={DIAL - 2 * RIM} radius="full" fill="fill" />
        <box at={[pointer(true), pointer(false)]} w={3} h={3} radius="full" fill="text" />
      </box>
      <box h={2} />
      <text style="label" color="text-secondary">{text ?? label}</text>
      <TypeIn id={id} label={label} value={value} unit={unit} range={range} w={CELL + 16} h={ROW - DIAL} drag={drag} onDragEnd={dragEnd} onChange={set}>
        {unit === undefined ? <text style="number" color="text" live={readout} digits={range.step && range.step >= 1 ? 0 : 2} /> : <text style="number" color="text" live={readout} unit={unit} />}
      </TypeIn>
    </col>
  );
};
