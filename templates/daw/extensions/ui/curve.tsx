import { h, type Color, type FeedList } from "../sdk";

export type CurveProps = {
  /** Heights evenly from the left edge to the right: a list feed, `live.feed("response")`, or numbers. */
  heights: FeedList | readonly number[];
  w: number;
  h: number;
  /** What the left and the right edge are, in the feed's units across, such as octaves above 20 Hz. */
  across: [number, number];
  /** What the top and the bottom are, such as `[18, -18]` dB. */
  range: [top: number, bottom: number];
  /** Filled softly down to the bottom, as a response; a wave is not. */
  filled?: boolean;
  color?: Color;
};

/** Numbers as a line from left to right, as `shapes` draws a list feed: segments end to end. */
const segments = (heights: readonly number[], [from, to]: [number, number]) => {
  const step = (to - from) / Math.max(heights.length - 1, 1);
  return heights.slice(1).flatMap((y, i) => [from + i * step, heights[i]!, from + (i + 1) * step, y]);
};

/**
 * The one bright line of a display, such as a response or a transfer curve, filled with a quiet
 * tint: drawn by the window as its feed moves, without the view.
 */
export const Curve = ({ heights, w, h: tall, across, range, filled = true, color = "text" }: CurveProps) => {
  const points = "node" in heights ? heights : segments(heights, across);
  return <shapes at={[0, 0]} w={w} h={tall} units={[across, range]} lines={[{ color, width: 1.5, points, ...(filled ? { fill: `${color}/5` as Color } : {}) }]} />;
};
