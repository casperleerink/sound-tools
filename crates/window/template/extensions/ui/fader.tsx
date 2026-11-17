import { h, live } from "../sdk";
import { TypeIn } from "./type-in";
import { fitted, liveValueAt, travelled, type Range } from "./value";

export const [MIN_DB, MAX_DB] = [-60, 6];
const RANGE: Range = { min: MIN_DB, max: MAX_DB, step: 0.1 };
const [THUMB, TARGET, TRACK, LINE] = [12, 28, 4, 24];

/**
 * A volume fader `height` points tall with its value under it: drag anywhere on it (with Shift,
 * ten times finer), arrows step a dB, a double click or Backspace sets 0 dB, a click on the value
 * or Enter types one. `onChange` hears the new dB when it is let go.
 */
export const Fader = ({ id, label, db, height, onChange }: { id: string; label: string; db: number; height: number; onChange: (db: number) => void }) => {
  const set = (to: number) => onChange(fitted(to, RANGE));
  const travel = height - THUMB;
  const at = travelled(fitted(db, RANGE), RANGE);
  const y = (1 - at) * travel;
  // A drag from the value line moves it too.
  const moved = live.drag(id).dy.plus(live.drag(`${id}-value`).dy);
  const readout = liveValueAt(moved.over(-travel).plus(at), RANGE);
  const drag = { axis: "y", min: -y, max: travel - y } as const;
  const dragEnd = ({ dy }: { dy: number }) => set(db - (dy / travel) * (MAX_DB - MIN_DB));
  return (
    <col key={id} label={label} role="slider" value={db} gap={2} align="center" radius="md" drag={drag} onDragEnd={dragEnd} actions={{ set, increment: (by) => set(db + by), decrement: (by) => set(db - by), reset: () => set(0) }}>
      <box w={TARGET} h={height}>
        <box at={[(TARGET - TRACK) / 2, 0]} w={TRACK} h={height} radius="full" fill="fill-strong" />
        <box at={[4, moved.plus(y)]} w={TARGET - 8} h={THUMB} radius="sm" fill="text" shadow="sm" />
      </box>
      <TypeIn id={id} label={label} value={db} unit="db" range={RANGE} w={64} h={LINE} drag={drag} onDragEnd={dragEnd} onChange={set}>
        <text style="number" color="text" live={readout} unit="db" />
      </TypeIn>
    </col>
  );
};
