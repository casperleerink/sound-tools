import { h, live, type Unit } from "../sdk";
import theme from "../theme.json";
import { liveValueAt, travelled, typedText, typedValue, valueAt, type Range } from "./value";

const [DIAL, CELL, ROW] = [theme.size.knob, theme.size.cell, theme.size.row];
/** Points of drag for the whole travel, and what an arrow key moves of it. */
const [TRAVEL, KEY_STEP] = [200, 0.02];
const [TRACK, FACE, SWEEP] = [2.5, 21, 270];
const RADIUS = DIAL / 2 - TRACK / 2;
/** The arc of the whole travel, as short segments from the bottom left round to the bottom right. */
const ARC: number[] = [];
for (let i = 0; i < 48; i++) {
  const angle = (t: number) => ((135 + t * SWEEP) * Math.PI) / 180;
  const [a, b] = [angle(i / 48), angle((i + 1) / 48)];
  ARC.push(DIAL / 2 + Math.cos(a) * RADIUS, DIAL / 2 + Math.sin(a) * RADIUS, DIAL / 2 + Math.cos(b) * RADIUS, DIAL / 2 + Math.sin(b) * RADIUS);
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
 * A knob in a cell of a card: the dial, its name and its value. Drag up or down anywhere on the
 * cell, arrows step it, a click or Enter types a value, a double click or Backspace sets
 * `initial`. The arc is the one bright line, and the value follows the drag as it goes.
 */
export const Knob = ({ id, label, text, value, initial, unit, bipolar = false, onChange, ...range }: KnobProps) => {
  const at = travelled(value, range);
  const set = (next: number) => onChange(valueAt(travelled(next, range), range));
  const step = (by: number) => (range.step ? set(value + by * range.step) : onChange(valueAt(at + by * KEY_STEP, range)));
  const dragged = live.drag(id).dy.over(-TRAVEL).plus(at);
  const angle = dragged.times((SWEEP * Math.PI) / 180).plus((135 * Math.PI) / 180);
  const pointer = (cos: boolean) => (cos ? angle.cos() : angle.sin()).times(FACE / 2 - 4).plus(DIAL / 2 - 1.5);
  const actions = { set, increment: (by: number) => step(by), decrement: (by: number) => step(-by), ...(initial === undefined ? {} : { reset: () => onChange(initial) }) };
  return (
    <col key={id} label={label} role="slider" value={value} w={CELL} h={ROW} align="center" radius="md" drag={{ axis: "y", min: -(1 - at) * TRAVEL, max: at * TRAVEL, move: false }} onDragEnd={({ dy }) => onChange(valueAt(at - dy / TRAVEL, range))} actions={actions}>
      <box w={DIAL} h={DIAL}>
        <shapes at={[0, 0]} w={DIAL} h={DIAL} lines={[{ color: "fill-strong", width: TRACK, points: ARC }]} />
        <shapes at={[0, 0]} w={DIAL} h={DIAL} lines={[{ color: "text", width: TRACK, points: ARC }]} trim={bipolar ? [dragged.min(0.5), dragged.max(0.5)] : [0, dragged]} />
        <box at={[(DIAL - FACE) / 2, (DIAL - FACE) / 2]} w={FACE} h={FACE} radius="full" fill="fill" />
        <box at={[pointer(true), pointer(false)]} w={3} h={3} radius="full" fill="text" />
      </box>
      <box h={2} />
      <text style="label" color="text-secondary">{text ?? label}</text>
      {unit === undefined ? <text style="number" color="text" live={liveValueAt(dragged, range)} digits={range.step && range.step >= 1 ? 0 : 2} /> : <text style="number" color="text" live={liveValueAt(dragged, range)} unit={unit} />}
      <row overlay={{ open: "press", side: "bottom", align: "center", gap: 38 - ROW }} w={CELL} h={28} px={1} align="center" radius="md" fill="raised" border="focus" shadow="md">
        <field key={`${id}-typed`} label={`Type ${label}`} value={typedText(value, unit)} style="number" align="center" w={CELL - 8} onCommit={(typed) => {
          const number = typedValue(typed, unit);
          if (number !== undefined) set(number);
        }} />
      </row>
    </col>
  );
};
