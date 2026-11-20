import { h, live, type Drag, type Live, type Pointer, type Unit } from "../sdk";
import theme from "../theme.json";
import { played } from "./field";
import { TypeIn } from "./type-in";
import { Tooltip } from "./tooltip";
import { fitted, liveShownAt, liveTravelled, liveValueAt, stepped, travelled, valueAt, type Range } from "./value";

const [DIAL, CELL, ROW] = [theme.size.knob, theme.size.cell, theme.size.row];
/** A small dial, as in a mixer's strip, and its value line. */
const [SMALL, LINE] = [24, 16];
/** Points of drag for the whole travel. */
const TRAVEL = 200;
/** The width of the arc, and from the edge of the dial to its face, as the window's own card has them. */
const [ARC, SWEEP] = [2.5, 270];
const rimOf = (dial: number) => (dial * 7.5) / 36;
/** The arc of the whole travel of a dial `dial` points wide, as short segments from the bottom left round to the bottom right. */
const arcOf = (dial: number) => {
  const radius = dial / 2 - ARC / 2;
  const points: number[] = [];
  for (let i = 0; i < 48; i++) {
    const angle = (t: number) => ((135 + t * SWEEP) * Math.PI) / 180;
    const [a, b] = [angle(i / 48), angle((i + 1) / 48)];
    points.push(dial / 2 + Math.cos(a) * radius, dial / 2 + Math.sin(a) * radius, dial / 2 + Math.cos(b) * radius, dial / 2 + Math.sin(b) * radius);
  }
  return points;
};
const ARCS = { [DIAL]: arcOf(DIAL), [SMALL]: arcOf(SMALL) };

export type KnobProps = Range & {
  id: string;
  /** The number of the record it turns (`cutoff_hz`, `t01/mix.gain_db`): the sound follows a drag. */
  path?: string;
  /** What it is called, for an agent; also shown under the dial unless `text` says it shorter. */
  label: string;
  text?: string;
  value: number;
  /** What a double click or Backspace sets. */
  initial?: number;
  unit?: Unit;
  /** Its arc starts from the middle, as a pan's. */
  bipolar?: boolean;
  /** What a lane plays into it, when one does: it shows that, with a dot and a tip that say why. */
  held?: Live;
  /** The value in the device's own words, such as a plugin's `3.2 kHz`: shown for the number while no lane holds it. */
  words?: string;
  /** A small dial without its name, as in a mixer's strip: its value under it, or `beside` it, `w` wide. */
  small?: { beside?: boolean; w: number; quiet?: boolean };
  onChange: (value: number) => void;
};

/**
 * A knob in a cell of a card: the dial, its name and its value. Drag up or down (with Shift,
 * ten times finer), arrows step it, a double click or Backspace sets `initial`, and a click on
 * the value or Enter types one. The arc is the one bright line and follows the drag.
 */
export const Knob = ({ id, path, label, text, value, initial, unit, bipolar = false, held, words, small, onChange, ...range }: KnobProps) => {
  const at = travelled(value, range);
  const set = (next: number) => onChange(fitted(next, range));
  // A drag from the value line turns the knob too.
  const turned = live.drag(id).dy.plus(live.drag(`${id}-value`).dy).over(-TRAVEL).plus(at);
  // A number of a record shows what plays: its own drag, a handle on it, a lane. A fader's -inf plays as -100 dB, so it shows its drag.
  const playing = path !== undefined && range.scale !== "fader" ? live.field(played(path)) : undefined;
  const dragged = held ? liveTravelled(held, range) : playing ? liveTravelled(playing, range) : turned;
  const drag: Drag = { y: [-(1 - at) * TRAVEL, at * TRAVEL], ...(path === undefined ? {} : { plays: { [path]: liveValueAt(turned, range) } }) };
  const dragEnd = ({ dy }: Pointer) => onChange(valueAt(at - dy / TRAVEL, range));
  const actions = { set, increment: (by: number) => onChange(stepped(value, by, range)), decrement: (by: number) => onChange(stepped(value, -by, range)), ...(initial === undefined ? {} : { reset: () => set(initial) }) };
  const readout = held ?? playing ?? liveShownAt(dragged, range);
  const number = words !== undefined && !held ? <text style="number" color="text" truncate>{words}</text>
    : unit === undefined ? <text style="number" color="text" live={readout} /> : <text style="number" color="text" live={readout} unit={unit} />;
  if (small) {
    const line = (
      <TypeIn id={id} label={label} value={value} unit={unit} range={range} w={small.w} h={LINE + 4} drag={drag} onDragEnd={dragEnd} onChange={set}>
        {number}
      </TypeIn>
    );
    const resting = small.quiet === true && value === initial && !held;
    return (
      <box key={id} label={label} role="slider" value={held ?? (value === -Infinity ? "-inf" : value)} radius="md" drag={drag} onDragEnd={dragEnd} actions={actions}>
        {h(small.beside ? "row" : "col", { gap: small.beside ? 1 : 0, align: "center" },
          <Dial at={dragged} bipolar={bipolar} mark={held !== undefined} size={SMALL} />,
          resting ? <box reveal>{line}</box> : line)}
        {held && <Tooltip text="A lane of the track plays this value" side="bottom" />}
      </box>
    );
  }
  return (
    <col key={id} label={label} role="slider" value={held ?? value} w={CELL} h={ROW} align="center" radius="md" drag={drag} onDragEnd={dragEnd} actions={actions}>
      <Dial at={dragged} bipolar={bipolar} mark={held !== undefined} />
      <box h={2} />
      <text style="label" color="text-secondary" w={CELL - 4} align="center" truncate>{text ?? label}</text>
      <TypeIn id={id} label={label} value={value} unit={unit} range={range} w={CELL + 16} h={ROW - DIAL} drag={drag} onDragEnd={dragEnd} onChange={set}>
        {number}
      </TypeIn>
      {held && <Tooltip text="A lane of the track plays this value" side="bottom" />}
    </col>
  );
};

/** The dial at `at` of the travel: the track, the arc to there, the face and its pointer. */
const Dial = ({ at, bipolar, mark = false, size = DIAL }: { at: Live; bipolar: boolean; mark?: boolean; size?: typeof DIAL | typeof SMALL }) => {
  const [rim, arc] = [rimOf(size), ARCS[size]!];
  const angle = at.times((SWEEP * Math.PI) / 180).plus((135 * Math.PI) / 180);
  const pointer = (cos: boolean) => (cos ? angle.cos() : angle.sin()).times(size / 2 - rim - 4).plus(size / 2 - 1.5);
  return (
    <box w={size} h={size}>
      <shapes at={[0, 0]} w={size} h={size} lines={[{ color: "fill-strong", width: ARC, points: arc }]} />
      <shapes at={[0, 0]} w={size} h={size} lines={[{ color: "text", width: ARC, points: arc }]} trim={bipolar ? [at.min(0.5), at.max(0.5)] : [0, at]} />
      <box at={[rim, rim]} w={size - 2 * rim} h={size - 2 * rim} radius="full" fill="fill" />
      <box at={[pointer(true), pointer(false)]} w={3} h={3} radius="full" fill="text" />
      {mark && <box at={[size - 5, 1]} w={4} h={4} radius="full" fill="text-secondary" />}
    </box>
  );
};
