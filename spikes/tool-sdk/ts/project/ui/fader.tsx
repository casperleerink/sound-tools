
const [MIN, MAX, THUMB] = [-60, 6, 12];

/** A volume fader `height` points tall: drag the thumb; `onChange` hears the new dB on release. */
export const Fader = ({ id, db, height, onChange }: { id: string; db: number; height: number; onChange: (db: number) => void }) => {
  const travel = height - THUMB;
  const y = (1 - (Math.min(Math.max(db, MIN), MAX) - MIN) / (MAX - MIN)) * travel;
  const dbAt = (top: number) => Math.round((MIN + (1 - top / travel) * (MAX - MIN)) * 10) / 10;
  return (
    <box w={24} h={height}>
      <box at={[10, 0]} w={4} h={height} radius="sm" fill="well" />
      <box key={id} at={[0, y]} w={24} h={THUMB} radius="sm" fill="text" shadow="sm" drag={{ axis: "y", min: -y, max: travel - y }} onDragEnd={({ dy }) => onChange(dbAt(y + dy))} />
    </box>
  );
};
