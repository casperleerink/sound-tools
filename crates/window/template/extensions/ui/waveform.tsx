import { h, type Color, type FeedList } from "../sdk";

/** What a card knows of a file its record names, as a tool's facts say it. */
export type FileState = { name: string; state: "plays" | "missing" | "does_not_play" | "unknown" };

export type WaveformProps = {
  /** The loudest of each stretch of the file, 0 to 1, from the left: a `wave` feed. */
  wave: FeedList;
  /** The seconds of the file across, as the feed reads them. */
  seconds: [from: number, to: number];
  file: FileState | null | undefined;
  w: number;
  h: number;
  /** What it says without a file, such as "Drop a sound here". */
  empty?: string;
  color?: Color;
};

/**
 * A file as its wave, mirrored about the middle, drawn by the window from the feed, inside a
 * `Display`. A file that is missing or does not play says so in words, where the wave would be.
 */
export const Waveform = ({ wave, seconds, file, w, h: tall, empty = "No sound yet", color = "text/24" }: WaveformProps) => {
  const said = !file ? empty : file.state === "missing" ? `${file.name} is missing` : file.state === "does_not_play" ? `${file.name} does not play` : undefined;
  if (said !== undefined)
    return (
      <row at={[0, 0]} w={w} h={tall} align="center" justify="center">
        <text style="label" color={file ? "warning" : "text-muted"} w={w - 16} align="center" truncate>{said}</text>
      </row>
    );
  const half = tall / 2;
  return (
    <box at={[0, 0]} w={w} h={tall}>
      <shapes at={[0, 0]} w={w} h={half} units={[seconds, [1, 0]]} lines={[{ color, width: 1, points: wave, fill: color }]} />
      {/* The lower half: the tint to the middle, and the well over it beyond the wave, which a line fills only downwards. */}
      <box at={[0, half]} w={w} h={half} fill={color} />
      <shapes at={[0, half]} w={w} h={half} units={[seconds, [0, 1]]} lines={[{ color, width: 1, points: wave, fill: "well" }]} />
    </box>
  );
};
