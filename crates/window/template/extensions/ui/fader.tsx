import { h, live, type Live } from "../sdk";
import { Tooltip } from "./tooltip";
import { TypeIn } from "./type-in";
import { fitted, liveTravelled, liveValueAt, travelled, type Range } from "./value";

export const [MIN_DB, MAX_DB] = [-60, 6];
const [THUMB, TARGET, TRACK, LINE] = [12, 28, 4, 24];

/**
 * A volume fader `height` points tall with its value under it: drag anywhere on it (with Shift,
 * ten times finer), arrows step a dB, a double click or Backspace sets 0 dB, a click on the value
 * or Enter types one. `onChange` hears the new dB when it is let go. While a lane plays into it,
 * `held`, it shows that and is not the composer's to change.
 */
export const Fader = ({ id, label, value: db, height, min = MIN_DB, held, onChange }: { id: string; label: string; value: number; height: number; min?: number; held?: Live; onChange: (db: number) => void }) => {
  const range: Range = { min, max: MAX_DB, step: 0.1 };
  const set = (to: number) => onChange(fitted(to, range));
  const travel = height - THUMB;
  const at = travelled(fitted(db, range), range);
  // A drag from the value line moves it too.
  const moved = live.drag(id).dy.plus(live.drag(`${id}-value`).dy);
  const shownAt = held ? liveTravelled(held, range) : moved.over(-travel).plus(at);
  const readout = held ?? liveValueAt(shownAt, range);
  const thumb = (
    <box w={TARGET} h={height}>
      <box at={[(TARGET - TRACK) / 2, 0]} w={TRACK} h={height} radius="full" fill="fill-strong" />
      <box at={[4, shownAt.negate().plus(1).times(travel)]} w={TARGET - 8} h={THUMB} radius="sm" fill="text" shadow="sm" />
      {held && <box at={[TARGET - 4, 0]} w={4} h={4} radius="full" fill="text-secondary" />}
    </box>
  );
  if (held) {
    return (
      <col key={id} label={label} role="slider" value={held} gap={2} align="center" radius="md">
        {thumb}
        <row w={64} h={LINE} justify="center"><text style="number" color="text" live={readout} unit="db" /></row>
        <Tooltip text="A lane of the track plays this value" side="bottom" />
      </col>
    );
  }
  const y = (1 - at) * travel;
  const drag = { axis: "y", min: -y, max: travel - y } as const;
  const dragEnd = ({ dy }: { dy: number }) => set(db - (dy / travel) * (MAX_DB - min));
  return (
    <col key={id} label={label} role="slider" value={db} gap={2} align="center" radius="md" drag={drag} onDragEnd={dragEnd} actions={{ set, increment: (by) => set(db + by), decrement: (by) => set(db - by), reset: () => set(0) }}>
      {thumb}
      <TypeIn id={id} label={label} value={db} unit="db" range={range} w={64} h={LINE} drag={drag} onDragEnd={dragEnd} onChange={set}>
        <text style="number" color="text" live={readout} unit="db" />
      </TypeIn>
    </col>
  );
};
