import { h, live, type Live } from "../sdk";
import { liveLevel, travelled, VOLUME } from "./value";

/** How fast a bar falls, in its height a second; how long the peak line stays before it falls. */
const [FALL, HOLD] = [0.6, 1.5];
/** Where yellow starts, on the fader's scale. */
const HOT = travelled(-6, VOLUME);
/** As long as a held peak stays: until a click lets go of it. */
const KEPT = 1e9;
/** The tallest clip light, and the room under it. */
export const CLIP = 4;

/** How often the meter `id` was clicked: a click lets go of its held peak and clip light. */
const clears = new Map<string, number>();
export const clearsOf = (id: string) => clears.get(id) ?? 0;
const clear = (id: string) => clears.set(id, clearsOf(id) + 1);

/** The loudest of `path` since the meter `id` was last clicked: a new value each click, so the window holds it anew. */
const kept = (id: string, path: string): Live => live.feed("peak", path).max(-clearsOf(id)).hold(KEPT);

export type MeterOf = { id: string; label: string; path: string };

/** What makes a box the meter `id`, for an agent, a screen reader and the keys: the level of `path`, which a click clears. */
export const meterOf = ({ id, label, path }: MeterOf) =>
  ({ key: id, label, role: "meter", value: live.feed("peak", path).decibels(), onPress: () => clear(id) }) as const;

/** The red light of the meter `id`, `w` × `h`, lit once `path` clipped, until a click. */
export const ClipLight = ({ id, path, at = [0, 0], w, h: tall }: { id: string; path: string; at?: [number, number]; w: number; h: number }) => (
  <box at={at} w={w} h={tall} radius="sm" fill="danger" opacity={kept(id, path).gt(1)} />
);

/**
 * The level of `path`, a bar a channel filling a well `h` tall from the bottom, on the fader's
 * scale: green, yellow from -6 dB, and a peak line that holds and falls. `quiet`, its well does
 * not show: in silence it is not there.
 */
export const Bars = ({ path, at, w, h: tall, quiet = false }: { path: string; at: [number, number]; w: number; h: number; quiet?: boolean }) => {
  const gap = w < 8 ? 1 : 2;
  const bar = (w - gap) / 2;
  const channel = (side: "peak_left" | "peak_right", x: number) => {
    const level = liveLevel(live.feed(side, path));
    const filled = level.fall(FALL).times(tall);
    const peak = level.hold(HOLD).fall(FALL).times(tall);
    const green = filled.min(HOT * tall);
    return [
      <box at={[x, 0]} w={bar} h={tall} fill={quiet ? undefined : "well"} />,
      <box at={[x, green.negate().plus(tall)]} w={bar} h={green} fill="sound" />,
      <box at={[x, filled.negate().plus(tall)]} w={bar} h={filled.minus(green).max(0)} fill="hot" />,
      <box at={[x, peak.negate().plus(tall - 1)]} w={bar} h={1} fill="text" opacity={peak.gt(1)} />,
    ];
  };
  return (
    <box at={at} w={w} h={tall}>
      {channel("peak_left", 0)}
      {channel("peak_right", bar + gap)}
    </box>
  );
};

/**
 * The level of the instance at `path` as {@link Bars}, under a red light once it clipped, until a
 * click. A meter is the one place colour fills an area.
 */
export const Meter = ({ id, label, path, w, h: height, quiet = false }: MeterOf & { w: number; h: number; quiet?: boolean }) => {
  const clip = Math.min(w, CLIP);
  return (
    <box {...meterOf({ id, label, path })} w={w} h={height} radius="sm" clip>
      <Bars path={path} at={[0, clip + 1]} w={w} h={height - clip - 1} quiet={quiet} />
      <ClipLight id={id} path={path} w={w} h={clip} />
    </box>
  );
};

/** The loudest the meter `id` heard since it was clicked, in dB: red once it clipped, none in silence. A click lets go of it. */
export const Peak = ({ id, label, path, w }: MeterOf & { w: number }) => {
  const loudest = kept(id, path);
  return (
    <row key={`${id}-peak`} label={label} role="button" w={w} justify="end" onPress={() => clear(id)}>
      <text style="number" color="text-secondary" live={loudest.decibels()} digits={1} opacity={loudest.gt(0.001).times(loudest.le(1))} />
      <text at={[0, 0]} style="number" color="danger" w={w} align="end" live={loudest.decibels()} digits={1} opacity={loudest.gt(1)} />
    </row>
  );
};
