import { h, live } from "../sdk";

export const [MIN_DB, MAX_DB] = [-60, 6];
const [THUMB, TARGET, TRACK] = [12, 28, 4];
const clamp = (db: number) => Math.min(Math.max(db, MIN_DB), MAX_DB);
const tenth = (db: number) => Math.round(clamp(db) * 10) / 10;

/**
 * A volume fader `height` points tall with its value under it: drag the thumb, arrows step a
 * dB, a double click or Backspace sets 0 dB. `onChange` hears the new dB when it is let go.
 */
export const Fader = ({ id, label, db, height, onChange }: { id: string; label: string; db: number; height: number; onChange: (db: number) => void }) => {
  const travel = height - THUMB;
  const range = MAX_DB - MIN_DB;
  const y = (1 - (clamp(db) - MIN_DB) / range) * travel;
  const moved = live.drag(id).dy;
  const dragged = moved.times(-range / travel).plus(clamp(db));
  return (
    <col gap={2} align="center">
      <box w={TARGET} h={height}>
        <box at={[(TARGET - TRACK) / 2, 0]} w={TRACK} h={height} radius="full" fill="fill-strong" />
        <row key={id} label={label} role="slider" value={db} at={[0, moved.plus(y - (TARGET - THUMB) / 2)]} w={TARGET} h={TARGET} align="center" justify="center" radius="md" drag={{ axis: "y", min: -y, max: travel - y }} onDragEnd={({ dy }) => onChange(tenth(db - (dy / travel) * range))} actions={{ set: (to) => onChange(tenth(to)), increment: (by) => onChange(tenth(db + by)), decrement: (by) => onChange(tenth(db - by)), reset: () => onChange(0) }}>
          <box w={20} h={THUMB} radius="sm" fill="text" shadow="sm" />
        </row>
      </box>
      <text style="number" color="text" live={dragged} unit="db" />
    </col>
  );
};
