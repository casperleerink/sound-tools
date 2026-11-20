import { h, live, type Child, type Live } from "../../sdk";
import { Bar } from "../bar";
import { Curve } from "../curve";
import { drawingOf } from "../display";

/** The room at the right of the drawing for the bar of how far it turns down, and over the curve. */
const [ROOM, INSET, BAR] = [18, 6, 5];
/** The most a bar shows it turned down, in dB. */
const MOST_DB = 24;

/** Where a dynamics display draws its curve and handles, `w` × `h` points: left of its bar, under its top. */
export const curveArea = () => {
  const { w, h: tall } = drawingOf(4);
  return { w: w - ROOM, h: tall - INSET };
};

/** Where `db` is, 0 to 1, on levels from `bottom` to `top` dBFS. */
export const partOf = (db: Live, [bottom, top]: [number, number]) => db.minus(bottom).over(top - bottom);

/**
 * In a `Display`: the bar of how far a compressor, gate or limiter turns the sound down now, at
 * the right, and while it does, "Turned down 6.8 dB" at the top left.
 */
export const TurnedDown = () => {
  const { w, h: tall } = drawingOf(4);
  const reduction = live.feed("reduction");
  return (
    <box at={[0, 0]} w={w} h={tall} inert>
      <box at={[w - (ROOM + BAR) / 2, INSET]} fill="fill"><Bar value={reduction} range={[0, MOST_DB]} w={BAR} h={tall - 2 * INSET} down /></box>
      <row at={[INSET, INSET]} h={18} px={1} gap={1} align="center" radius="sm" fill="well" opacity={reduction.gt(0.05)}>
        <text style="number" color="text-secondary">Turned down</text>
        <text style="number" color="text-secondary" live={reduction.times(10).round().over(10)} unit="db" />
      </row>
    </box>
  );
};

/**
 * What a compressor or gate does to each level, `levels` [bottom, top] dBFS in and out: its
 * `transfer` curve, a faint line at the threshold, the level it hears as a green dot on the
 * curve, how far it turns down, and its `handles` on the curve. Up to the level in `floor`, the
 * curve is under the drawing: it lies along the bottom.
 */
export const DynamicsCurve = ({ levels, floor, handles }: { levels: [bottom: number, top: number]; floor?: Live; handles: Child }) => {
  const { w, h: tall } = curveArea();
  const [level, reduction] = [live.feed("level"), live.feed("reduction")];
  return (
    <box at={[0, 0]}>
      <box at={[0, INSET]} w={w} h={tall}>
        <box at={[partOf(live.field("threshold_db"), levels).times(w), 0]} w={1} h={tall} fill="border" />
        <Curve heights={live.feed("transfer")} w={w} h={tall} across={levels} range={[levels[1], levels[0]]} />
        {floor && <box at={[0, tall - 1.5]} w={partOf(floor, levels).clamp(0, 1).times(w)} h={1.5} fill="text" />}
        <box at={[partOf(level, levels).clamp(0, 1).times(w).minus(4), partOf(level.minus(reduction), levels).clamp(0, 1).negate().plus(1).times(tall).minus(4)]}
          w={8} h={8} radius="full" fill="green" opacity={level.gt(levels[0])} />
        {handles}
      </box>
      <TurnedDown />
    </box>
  );
};
