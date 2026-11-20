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

/**
 * A volume fader `height` points tall, on the fader's scale: silence at the bottom, 0 dB well up.
 * Drag anywhere on it (with Shift, ten times finer), arrows step it, a double click or Backspace
 * sets 0 dB, Enter types a value. `onChange` hears the new dB when it is let go; the sound of
 * `path` follows the drag. While a lane plays into it, `held`, it shows that, with a dot and a tip
 * that say why. Its value line is a {@link FaderValue}, under it unless `bare`.
 */
export const Fader = (props: FaderProps & { bare?: boolean }) => {
  const { id, label, value, height, held, bare = false } = props;
  const { range, travel, shownAt, drag, dragEnd, actions } = faderOf(props);
  const y = (db: number) => (1 - travelled(db, range)) * travel + CAP / 2;
  const mark = (db: number) => (
    <box at={[0, y(db)]} w={TARGET} h={1} fill={db === 0 ? "text-muted" : "border-strong"} />
  );
  return (
    <col gap={2} align="center">
      <box key={id} label={label} role="slider" value={held ?? (value === -Infinity ? "-inf" : value)} w={TARGET} h={height} radius="sm" drag={drag} onDragEnd={dragEnd} actions={actions}>
        {MARKS.filter((db) => db <= range.max && db >= range.min).map(mark)}
        <box at={[(TARGET - TRACK) / 2, 0]} w={TRACK} h={height} radius="full" fill="well" />
        <box at={[(TARGET - TRACK) / 2, shownAt.negate().plus(1).times(travel).plus(CAP / 2)]} w={TRACK} h={shownAt.times(travel)} radius="full" fill="fill-strong" />
        <box at={[2, shownAt.negate().plus(1).times(travel)]} w={TARGET - 4} h={CAP} radius="sm" fill={held ? "text-secondary" : "text"} shadow="sm">
          <box at={[4, CAP / 2 - 0.5]} w={TARGET - 12} h={1} fill="text-inverse" />
        </box>
        {held && <box at={[TARGET - 4, 0]} w={4} h={4} radius="full" fill="text-secondary" />}
        {held && <Tooltip text="A lane of the track plays this value" side="bottom" />}
      </box>
      {!bare && <FaderValue {...props} w={64} />}
    </col>
  );
};

/** The value line of a {@link Fader} of the same props, `w` wide: a click types a value (`-inf` silences it), a drag moves the fader. `quiet`: it shows at 0 dB only under the pointer. */
export const FaderValue = (props: FaderProps & { w: number; quiet?: boolean }) => {
  const { id, label, value, held, w, quiet = false } = props;
  const { range, set, drag, dragEnd, readout } = faderOf(props);
  const line = (
    <TypeIn id={id} label={label} value={value} unit="db" range={range} w={w} h={LINE + 4} drag={drag} onDragEnd={dragEnd} onChange={set}>
      <text style="number" color={value === 0 && !held ? "text-secondary" : "text"} live={readout} unit="db" />
    </TypeIn>
  );
  return quiet && value === 0 && !held ? <box reveal>{line}</box> : line;
};
