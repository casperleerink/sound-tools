import { h, live } from "../sdk";
import { fitted, stepped, travelled, valueAt, type Range } from "./value";

const [THUMB, TARGET] = [12, 24];

/** A value along a line `w` points long, for a page rather than a card: drag the thumb (with Shift, ten times finer), arrows step it. */
export const Slider = ({ id, label, value, w, onChange, ...range }: Range & { id: string; label: string; value: number; w: number; onChange: (value: number) => void }) => {
  const travel = w - THUMB;
  const at = travelled(value, range);
  const x = at * travel;
  const dragged = live.drag(id).dx.plus(x);
  const set = (to: number) => onChange(fitted(to, range));
  return (
    <box w={w} h={TARGET}>
      <box at={[0, (TARGET - 2) / 2]} w={w} h={2} radius="full" fill="fill-strong" />
      <box at={[0, (TARGET - 2) / 2]} w={dragged.plus(THUMB / 2)} h={2} radius="full" fill="text" />
      <row key={id} label={label} role="slider" value={value} at={[dragged.minus((TARGET - THUMB) / 2), 0]} w={TARGET} h={TARGET} align="center" justify="center" radius="full" drag={{ axis: "x", min: -x, max: travel - x }} onDragEnd={({ dx }) => onChange(valueAt((x + dx) / travel, range))} actions={{ set, increment: (by) => onChange(stepped(value, by, range)), decrement: (by) => onChange(stepped(value, -by, range)) }}>
        <box w={THUMB} h={THUMB} radius="full" fill="text" shadow="sm" />
      </row>
    </box>
  );
};
