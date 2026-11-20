import { h, live } from "../sdk";

/** The quietest a meter shows and where yellow starts, in dB, and how fast it falls, in dB a second. */
const [FLOOR, HOT, FALL] = [-60, -6, 24];

/**
 * The level of the instance at `path`, filling a well from the bottom: green, yellow from -6 dB,
 * and a red light on top once it clipped. A meter is the one place colour fills an area.
 */
export const Meter = ({ id, label, path, w, h: height }: { id: string; label: string; path: string; w: number; h: number }) => {
  const level = live.feed("peak", path).decibels().fall(FALL);
  const at = (db: number) => ((db - FLOOR) / -FLOOR) * height;
  const filled = level.minus(FLOOR).over(-FLOOR).clamp(0, 1).times(height);
  const green = filled.min(at(HOT));
  const hot = filled.minus(at(HOT)).max(0);
  return (
    <box key={`${id}-well`} label={label} role="meter" w={w} h={height} radius="sm" fill="well" clip>
      <box key={id} at={[0, green.negate().plus(height)]} w={w} h={green} fill="sound" />
      <box at={[0, filled.negate().plus(height)]} w={w} h={hot} fill="hot" />
      <box at={[0, 0]} w={w} h={2} fill="danger" opacity={live.feed("peak", path).hold(2).gt(1)} />
    </box>
  );
};
