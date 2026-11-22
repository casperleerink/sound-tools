import { h, live, type Live, type Pointer } from "../sdk";
import { Tooltip } from "./tooltip";
import { fitted, liveTravelled, liveValueAt, stepped, travelled, valueAt, type Range } from "./value";

const [THUMB, TARGET] = [12, 24];

/**
 * A value along a line `w` points long, for a page rather than a card: drag the thumb (with
 * Shift, ten times finer), arrows step it; the sound of `path` follows the drag. While a lane plays into it, `held`, it shows that, and
 * a tip says why.
 */
export const Slider = ({ id, path, label, value, w, held, onChange, ...range }: Range & { id: string; path?: string; label: string; value: number; w: number; held?: Live; onChange: (value: number) => void }) => {
  const travel = w - THUMB;
  const at = travelled(value, range);
  const x = at * travel;
  const slid = live.drag(id).dx.plus(x);
  const dragged = held ? liveTravelled(held, range).times(travel) : slid;
  const plays = path === undefined ? {} : { plays: { [path]: liveValueAt(slid.over(travel), range) } };
  const set = (to: number) => onChange(fitted(to, range));
  return (
    <box w={w} h={TARGET}>
      <box at={[0, (TARGET - 2) / 2]} w={w} h={2} radius="full" fill="fill-strong" />
      <box at={[0, (TARGET - 2) / 2]} w={dragged.plus(THUMB / 2)} h={2} radius="full" fill="text" />
      <row key={id} label={label} role="slider" value={held ?? value} at={[dragged.minus((TARGET - THUMB) / 2), 0]} w={TARGET} h={TARGET} align="center" justify="center" radius="full"
        drag={{ x: [-x, travel - x], ...plays }} onDragEnd={({ dx }: Pointer) => onChange(valueAt((x + dx) / travel, range))} actions={{ set, increment: (by) => onChange(stepped(value, by, range)), decrement: (by) => onChange(stepped(value, -by, range)) }}>
        <box w={THUMB} h={THUMB} radius="full" fill={held ? "text-secondary" : "text"} shadow="sm" />
        {held && <Tooltip text="A lane of the track plays this value" />}
      </row>
    </box>
  );
};
