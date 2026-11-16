import { h } from "../sdk/view";

const SIZE = 32;
const TRAVEL = 120;
const SWEEP = 270;

/** A knob of a number from `min` to `max`: drag up or down; `onChange` hears the value when it is let go. The arc is the one bright line. */
export const Knob = ({ id, label, value, min, max, onChange }: { id: string; label: string; value: number; min: number; max: number; onChange: (value: number) => void }) => {
  const at = (value - min) / (max - min || 1);
  const set = (next: number) => onChange(Math.min(Math.max(next, min), max));
  // The arc as short segments, in degrees from the bottom left.
  const points: number[] = [];
  const track: number[] = [];
  const steps = 24;
  for (let i = 0; i < steps; i++) {
    const [a, b] = [i / steps, (i + 1) / steps];
    const angle = (t: number) => ((135 + t * SWEEP) * Math.PI) / 180;
    const seg = [Math.cos(angle(a)), Math.sin(angle(a)), Math.cos(angle(b)), Math.sin(angle(b))].map((v) => v * 13 + SIZE / 2);
    (b <= at ? points : track).push(...seg);
  }
  return (
    <col gap={1} align="center">
      <box
        key={id}
        label={label}
        role="slider"
        value={value}
        w={SIZE}
        h={SIZE}
        drag={{ axis: "y", min: -TRAVEL, max: TRAVEL }}
        onDragEnd={({ dy }) => set(value - (dy / TRAVEL) * (max - min))}
        actions={{ set, increment: () => set(value + (max - min) / 100), decrement: () => set(value - (max - min) / 100) }}
      >
        <shapes at={[0, 0]} w={SIZE} h={SIZE} lines={[{ color: "fill-strong", width: 3, points: track }, { color: "text", width: 3, points }]} />
      </box>
      <text style="label" color="text-secondary">{label}</text>
    </col>
  );
};
