import { h, live } from "../sdk";
import { TypeIn } from "./type-in";
import { fitted, type Range } from "./value";

export const [MIN_DB, MAX_DB] = [-60, 6];
const RANGE: Range = { min: MIN_DB, max: MAX_DB, step: 0.1 };
const [THUMB, TARGET, TRACK] = [12, 28, 4];

/**
 * A volume fader `height` points tall with its value under it: drag the thumb (with Shift, ten
 * times finer), arrows step a dB, a double click or Backspace sets 0 dB, a click on the value or
 * Enter types one. `onChange` hears the new dB when it is let go.
 */
export const Fader = ({ id, label, db, height, onChange }: { id: string; label: string; db: number; height: number; onChange: (db: number) => void }) => {
  const set = (to: number) => onChange(fitted(to, RANGE));
  const travel = height - THUMB;
  const range = MAX_DB - MIN_DB;
  const y = (1 - (fitted(db, RANGE) - MIN_DB) / range) * travel;
  const moved = live.drag(id).dy;
  const dragged = moved.times(-range / travel).plus(fitted(db, RANGE));
  return (
    <col key={`${id}-strip`} gap={2} align="center">
      <box w={TARGET} h={height}>
        <box at={[(TARGET - TRACK) / 2, 0]} w={TRACK} h={height} radius="full" fill="fill-strong" />
        <row key={id} label={label} role="slider" value={db} at={[0, moved.plus(y - (TARGET - THUMB) / 2)]} w={TARGET} h={TARGET} align="center" justify="center" radius="md" drag={{ axis: "y", min: -y, max: travel - y }} onDragEnd={({ dy }) => set(db - (dy / travel) * range)} actions={{ set, increment: (by) => set(db + by), decrement: (by) => set(db - by), reset: () => set(0) }}>
          <box w={20} h={THUMB} radius="sm" fill="text" shadow="sm" />
        </row>
      </box>
      <TypeIn id={id} label={label} value={db} unit="db" range={RANGE} w={64} h={24} onChange={set}>
        <text style="number" color="text" live={dragged} unit="db" />
      </TypeIn>
    </col>
  );
};
