import { h, live, type Color } from "../../../sdk";
import { controllerName, LANE_NAMES, pointsOf, type NoteLane } from "../../../song/notes";
import { LANE, playheadOf, type Roll } from "./roll-view";

type PointLane = Exclude<NoteLane, "velocity">;

/**
 * An expression lane of the clip, such as the pedal or the bend: a line through its points, flat
 * before the first and after the last. A double click adds a point on the snap, a drag moves one,
 * Alt and a press takes one away. Down the lane is its travel, 0 at the top.
 */
export const ExpressionLane = ({ roll, lane }: { roll: Roll; lane: PointLane }) => {
  const key = `${roll.key}-points`;
  const name = typeof lane === "number" ? controllerName(lane).label : LANE_NAMES[lane];
  const [low, high] = lane === "bend" ? [-8192, 8191] : [0, 127];
  const down = (value: number) => 1 - (value - low) / (high - low);
  const points = pointsOf(roll.state, lane);
  const snap = roll.page.song.memory.snap;
  const heights = points.map((point) => down(point.value));
  const path = points.length === 0 ? [] : [0, heights[0]!, ...points.flatMap((point, at) => [point.at, heights[at]!]), roll.end, heights.at(-1)!];
  const segments = path.flatMap((_, at) => (at % 2 === 0 && at + 3 < path.length ? path.slice(at, at + 4) : []));
  const { at, inside } = playheadOf(roll);
  return (
    <box key={key} label={`${name} lane`} grow={1} h={LANE - 1} clip plane={{ x: roll.key, y: [-0.12, 1.12] }}
      onPress={(p) => p.clicks === 2 && roll.edits.addPoint(lane, p)}>
      {lane === "bend" && <shapes place={[0, 0, roll.end, 1]} lines={[{ color: "border", width: 1, points: [0, 0.5, roll.end, 0.5] }]} />}
      <shapes place={[0, 0, roll.end, 1]} lines={[{ color: roll.colour, width: 2, points: segments, fill: `${roll.colour}/15` as Color }]} />
      {points.map((point, index) => {
        const drag = live.drag(`point-${index}`);
        return (
          <box key={`point-${index}`} label={`Point ${index + 1} of ${name}`} role="button" value={point.value} place={[drag.dx.plus(point.at), drag.dy.plus(heights[index]!)]} w={10} h={10} radius="full" fill="text" border={roll.colour}
            drag={{ x: true, y: [-heights[index]!, 1 - heights[index]!], snap: [snap, 0], from: [point.at, heights[index]!] }}
            onPress={(p) => p.alt && roll.edits.removePoint(lane, point.at)} onDragEnd={(p) => roll.edits.movePoint(lane, point.at, p)} />
        );
      })}
      {points.length === 0 && (
        <row place={[0, 0, roll.end, 1]} px={3} align="center" inert>
          <text style="label" color="text-muted">Double-click to add a point</text>
        </row>
      )}
      <box place={[at, 0.5]} w={1} h={1000} fill="text" opacity={inside} inert />
    </box>
  );
};
