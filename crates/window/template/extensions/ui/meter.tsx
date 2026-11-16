import { h, live } from "../sdk";

/** The quietest a meter shows, in dB, and how fast it falls after the sound, in dB a second. */
const [FLOOR, FALL] = [-60, 24];

/** The level of the instance at `path`, filling a well from the bottom. Green: a meter is the one place colour fills an area. */
export const Meter = ({ id, label, path, w, h: height }: { id: string; label: string; path: string; w: number; h: number }) => {
  const filled = live.peak(path).decibels().fall(FALL).minus(FLOOR).over(-FLOOR).clamp(0, 1).times(height);
  return (
    <box key={`${id}-well`} label={label} role="meter" w={w} h={height} radius="sm" fill="well" clip>
      <box key={id} at={[0, filled.negate().plus(height)]} w={w} h={filled} fill="green" />
    </box>
  );
};
