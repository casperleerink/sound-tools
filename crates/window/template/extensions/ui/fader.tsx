import { h } from "../sdk/view";

export const [MIN_DB, MAX_DB] = [-60, 6];
const THUMB = 12;
const clamp = (db: number) => Math.min(Math.max(db, MIN_DB), MAX_DB);

/** A volume fader `height` points tall: drag the thumb; `onChange` hears the new dB when it is let go. */
export const Fader = ({ id, label, db, height, onChange }: { id: string; label: string; db: number; height: number; onChange: (db: number) => void }) => {
  const travel = height - THUMB;
  const y = (1 - (clamp(db) - MIN_DB) / (MAX_DB - MIN_DB)) * travel;
  const dbAt = (top: number) => Math.round(clamp(MIN_DB + (1 - top / travel) * (MAX_DB - MIN_DB)) * 10) / 10;
  const set = (value: number) => onChange(Math.round(clamp(value) * 10) / 10);
  return (
    <box w={20} h={height}>
      <box at={[8, 0]} w={4} h={height} radius="full" fill="fill-strong" />
      <box
        key={id}
        label={label}
        role="slider"
        value={db}
        at={[0, y]}
        w={20}
        h={THUMB}
        radius="sm"
        fill="text"
        shadow="sm"
        drag={{ axis: "y", min: -y, max: travel - y }}
        onDragEnd={({ dy }) => set(dbAt(y + dy))}
        actions={{ set, increment: () => set(db + 1), decrement: () => set(db - 1) }}
      />
    </box>
  );
};
