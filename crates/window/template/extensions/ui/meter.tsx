import { h, live, Live } from "../sdk";
import { liveLevel, travelled, VOLUME } from "./value";

/**
 * The loudest sample of a channel of `path` since the last frame. The window reads no feed by its
 * name yet, so not `live.feed("peak_left", path)`: what it read before feeds, a peak of a channel.
 */
const channelPeak = (path: string, channel: 0 | 1) => new Live({ op: "peak", name: path, channel });

/** How fast a bar falls, in its height a second; how long the peak line stays before it falls. */
const [FALL, HOLD] = [0.6, 1.5];
/** Where yellow starts, on the fader's scale. */
const HOT = travelled(-6, VOLUME);
/** As long as a held peak stays: until a click lets go of it. */
const KEPT = 1e9;

/** How often the meter `id` was clicked: a click lets go of its held peak and clip light. */
const clears = new Map<string, number>();
export const clearsOf = (id: string) => clears.get(id) ?? 0;

/** The loudest of `path` since the meter `id` was last clicked: a new value each click, so the window holds it anew. */
const kept = (id: string, path: string): Live => live.feed("peak", path).max(-clearsOf(id)).hold(KEPT);

/**
 * The level of the instance at `path`, a bar a channel filling a well from the bottom, on the
 * fader's scale so it reads against a fader beside it: green, yellow from -6 dB, a peak line that
 * holds and falls, and a red light on top once it clipped, until a click. A meter is the one place
 * colour fills an area.
 */
export const Meter = ({ id, label, path, w, h: height }: { id: string; label: string; path: string; w: number; h: number }) => {
  const gap = w < 8 ? 1 : 2;
  const bar = (w - gap) / 2;
  const clip = Math.min(bar * 2 + gap, 4);
  const channel = (side: 0 | 1, x: number) => {
    const level = liveLevel(channelPeak(path, side));
    const filled = level.fall(FALL).times(height - clip - 1);
    const peak = level.hold(HOLD).fall(FALL).times(height - clip - 1);
    const green = filled.min(HOT * (height - clip - 1));
    const top = height;
    return [
      <box at={[x, clip + 1]} w={bar} h={height - clip - 1} fill="well" />,
      <box at={[x, green.negate().plus(top)]} w={bar} h={green} fill="sound" />,
      <box at={[x, filled.negate().plus(top)]} w={bar} h={filled.minus(green).max(0)} fill="hot" />,
      <box at={[x, peak.negate().plus(top - 1)]} w={bar} h={1} fill="text" opacity={peak.gt(1)} />,
    ];
  };
  return (
    <box key={id} label={label} role="meter" value={live.feed("peak", path).decibels()} w={w} h={height} radius="sm" clip onPress={() => clear(id)}>
      {channel(0, 0)}
      {channel(1, bar + gap)}
      <box at={[0, 0]} w={w} h={clip} radius="sm" fill="danger" opacity={kept(id, path).gt(1)} />
    </box>
  );
};

/** The loudest the meter `id` heard since it was clicked, in dB: red once it clipped, none in silence. A click lets go of it. */
export const Peak = ({ id, label, path, w }: { id: string; label: string; path: string; w: number }) => {
  const loudest = kept(id, path);
  return (
    <row key={`${id}-peak`} label={label} role="button" w={w} justify="end" onPress={() => clear(id)}>
      <text style="number" color="text-secondary" live={loudest.decibels()} digits={1} opacity={loudest.gt(0.001).times(loudest.le(1))} />
      <text at={[0, 0]} style="number" color="danger" w={w} align="end" live={loudest.decibels()} digits={1} opacity={loudest.gt(1)} />
    </row>
  );
};

const clear = (id: string) => clears.set(id, clearsOf(id) + 1);
