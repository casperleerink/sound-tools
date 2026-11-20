import { h, live, type Drag, type Live, type Pointer } from "../sdk";
import { Tooltip } from "./tooltip";
import { TypeIn } from "./type-in";
import { fitted, liveShownAt, liveTravelled, liveValueAt, stepped, travelled, valueAt, VOLUME, type Range } from "./value";

const [CAP, TARGET, TRACK, LINE] = [10, 28, 4, 20];
/** The marks beside the track, in dB: unity brighter. */
const MARKS = [6, 0, -6, -12, -24, -48];

export type FaderProps = Partial<Range> & {
  id: string;
  /** The number of the record it moves, such as `t00/mix.gain_db`: the sound follows a drag. */
  path?: string;
  label: string;
  /** In dB, -Infinity for silence. */
  value: number;
  /** How far it travels, in points. */
  height: number;
  /** What a lane plays into it, when one does: it shows that, with a dot and a tip that say why. */
  held?: Live;
  onChange: (db: number) => void;
};

/** What a fader and its value line share: one drag, one value. */
const faderOf = ({ id, path, value, height, held, onChange, ...given }: FaderProps) => {
  const range: Range = { ...VOLUME, ...given };
  const set = (to: number) => onChange(fitted(to, range));
  const travel = height - CAP;
  const at = travelled(value, range);
  // A drag from the value line moves it too.
  const movedAt = live.drag(id).dy.plus(live.drag(`${id}-value`).dy).over(-travel).plus(at);
  const shownAt = held ? liveTravelled(held, range) : movedAt;
  const drag: Drag = { y: [-(1 - at) * travel, at * travel], ...(path === undefined ? {} : { plays: { [path]: liveValueAt(movedAt, range) } }) };
  const dragEnd = ({ dy }: Pointer) => set(valueAt(at - dy / travel, range));
  const actions = { set, increment: (by: number) => set(stepped(value, by, range)), decrement: (by: number) => set(stepped(value, -by, range)), reset: () => set(0) };
  return { range, set, travel, shownAt, readout: held ?? liveShownAt(movedAt, range), drag, dragEnd, actions };
};

/** Where its value line is under it: from `x` points left of the fader's own left, `w` wide; `quiet`, it shows at 0 dB only under the pointer. */
export type ValueLine = { x: number; w: number; quiet?: boolean };
/** How far the value line is under the fader, and how tall it is. */
export const [UNDER, LINE_HEIGHT] = [4, LINE + 4];

/**
 * A volume fader `height` points tall, on the fader's scale: silence at the bottom, 0 dB well up,
 * and its value line under it, which a click or Enter types into (`-inf` silences it). Drag
 * anywhere on it (with Shift, ten times finer), arrows step it, a double click or Backspace sets
 * 0 dB. `onChange` hears the new dB when it is let go; the sound of `path` follows the drag. While
 * a lane plays into it, `held`, it shows that, with a dot and a tip that say why.
 */
export const Fader = (props: FaderProps & { line?: ValueLine }) => {
  const { id, label, value, height, held, line = { x: (64 - TARGET) / 2, w: 64 } } = props;
  const { range, set, travel, shownAt, readout, drag, dragEnd, actions } = faderOf(props);
  const y = (db: number) => (1 - travelled(db, range)) * travel + CAP / 2;
  const mark = (db: number) => <box at={[0, y(db)]} w={TARGET} h={1} fill={db === 0 ? "text-muted" : "border-strong"} />;
  const typed = (
    <TypeIn id={id} label={label} value={value} unit="db" range={range} w={line.w} h={LINE_HEIGHT} drag={drag} onDragEnd={dragEnd} onChange={set}>
      <text style="number" color={value === 0 && !held ? "text-secondary" : "text"} live={readout} unit="db" />
    </TypeIn>
  );
  return (
    <box key={id} label={label} role="slider" value={held ?? (value === -Infinity ? "-inf" : value)} w={TARGET} h={height + UNDER + LINE_HEIGHT} drag={drag} onDragEnd={dragEnd} actions={actions}>
      {MARKS.filter((db) => db <= range.max && db >= range.min).map(mark)}
      <box at={[(TARGET - TRACK) / 2, 0]} w={TRACK} h={height} radius="full" fill="well" />
      <box at={[(TARGET - TRACK) / 2, shownAt.negate().plus(1).times(travel).plus(CAP / 2)]} w={TRACK} h={shownAt.times(travel)} radius="full" fill="fill-strong" />
      <box at={[2, shownAt.negate().plus(1).times(travel)]} w={TARGET - 4} h={CAP} radius="sm" fill={held ? "text-secondary" : "text"} shadow="sm">
        <box at={[4, CAP / 2 - 0.5]} w={TARGET - 12} h={1} fill="text-inverse" />
      </box>
      {held && <box at={[TARGET - 4, 0]} w={4} h={4} radius="full" fill="text-secondary" />}
      <box at={[-line.x, height + UNDER]} w={line.w} h={LINE_HEIGHT} {...(line.quiet && value === 0 && !held ? { reveal: true } : {})}>{typed}</box>
      {held && <Tooltip text="A lane of the track plays this value" side="bottom" />}
    </box>
  );
};
