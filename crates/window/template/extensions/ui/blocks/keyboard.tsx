import { h, live } from "../../sdk";

const [WHITE, BLACK, TALL, SHORT] = [24, 16, 96, 60];
const NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
const black = (pitch: number) => [1, 3, 6, 8, 10].includes(pitch % 12);
const nameOf = (pitch: number) => `${NAMES[pitch % 12]}${Math.floor(pitch / 12) - 1}`;

/**
 * Piano keys from `low` to `high` (60 is C4): a press plays one, and a key lights green while it
 * sounds on the tool at `path`, whoever plays it. One stop of Tab; the arrow keys move.
 */
export const Keyboard = ({ id, low, high, path, onPlay }: { id: string; low: number; high: number; path?: string; onPlay: (pitch: number) => void }) => {
  const pitches = Array.from({ length: high - low + 1 }, (_, i) => low + i);
  const whites = pitches.filter((pitch) => !black(pitch));
  const key = (pitch: number, x: number, w: number, tall: number, fill: "text" | "well") => (
    <box key={`${id}-${pitch}`} label={nameOf(pitch)} role="button" at={[x, 0]} w={w} h={tall} radius="sm" fill={fill} border="background" clip onPress={() => onPlay(pitch)}>
      <box at={[0, 0]} w={w} h={tall} fill="sound" opacity={live.sounding(pitch, path).times(0.8)} inert />
      {pitch % 12 === 0 && <text at={[4, tall - 18]} style="label" color="text-inverse">{nameOf(pitch)}</text>}
    </box>
  );
  return (
    <box key={id} label="Keys" role="group" group w={whites.length * WHITE} h={TALL}>
      {whites.map((pitch, i) => key(pitch, i * WHITE, WHITE, TALL, "text"))}
      {pitches.filter(black).map((pitch) => key(pitch, whites.filter((white) => white < pitch).length * WHITE - BLACK / 2, BLACK, SHORT, "well"))}
    </box>
  );
};
