import { h, Live, type Color } from "../sdk";

/** How long the line of the most stays, then how fast it falls, in the bar's length a second. */
const [HOLD, FALL] = [1.5, 0.6];

export type BarProps = {
  /** A number feed, such as `live.feed("reduction")`. */
  value: Live;
  /** What an empty and a full bar are, such as `[0, 24]` dB. */
  range: [empty: number, full: number];
  w: number;
  h: number;
  /** Hangs from the top, as how far a compressor turns the sound down; else stands up from the bottom, as a level. */
  down?: boolean;
  /** A soft fill: green is sound that moves, a reduction is quiet. */
  color?: Color;
  /** A thin line at the most it reached, which stays a moment and falls. */
  hold?: boolean;
};

/**
 * A number that moves as a bar inside a display or beside it, such as a level or a gain
 * reduction: the window draws it every frame, without the view.
 */
export const Bar = ({ value, range: [empty, full], w, h: tall, down = false, color = "text/24", hold = false }: BarProps) => {
  const length = value.minus(empty).over(full - empty).clamp(0, 1).times(tall);
  const held = length.hold(HOLD).fall(FALL * tall);
  const from = (part: Live) => (down ? Live.of(0) : part.negate().plus(tall));
  return (
    <box w={w} h={tall}>
      <box at={[0, from(length)]} w={w} h={length} fill={color} />
      {hold && <box at={[0, down ? held.minus(1).max(0) : held.negate().plus(tall)]} w={w} h={1} fill="text" opacity={held.gt(1)} />}
    </box>
  );
};
