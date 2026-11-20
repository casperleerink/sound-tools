import { h, live, type Dropped, type Live } from "../sdk";
import theme from "../theme.json";

export type PadProps = {
  id: string;
  label: string;
  text?: string;
  /** What it plays, under its name, such as the file of a sample. */
  detail?: string;
  pitch: number;
  path?: string;
  /** How loud it sounds, 0 to 1, such as one of a drum pad's `levels`: else whether its pitch sounds. */
  level?: Live;
  /** Its file is not there: a warning sign. */
  missing?: boolean;
  chosen?: boolean;
  w?: number;
  h?: number;
  onPress: () => void;
  /** A file dropped on it from the system. */
  onDrop?: (dropped: Dropped) => void;
};

/**
 * A drum pad that plays `pitch` on a press and lights green while it sounds, whoever plays it.
 * `chosen` is the pad whose sound the card shows.
 */
export const Pad = ({ id, label, text, detail, pitch, path, level, missing = false, chosen = false, w = theme.size.cell, h: tall = theme.size.cell, onPress, onDrop }: PadProps) => (
  <box key={id} label={label} role="button" w={w} h={tall} radius="md" fill={chosen ? "selected" : "fill"} hover="fill" clip onPress={onPress} {...(onDrop ? { onDrop } : {})}>
    <box at={[0, 0]} w={w} h={tall} fill="sound/24" opacity={level ?? live.feed(`sounding.${pitch}`, path)} inert />
    <text at={[8, 6]} w={w - 16} style="label" color={chosen ? "text" : "text-secondary"} truncate>{text ?? label}</text>
    {detail !== undefined && <text at={[8, tall - 22]} w={w - (missing ? 28 : 16)} style="label" color="text-muted" truncate>{detail}</text>}
    {missing && <box at={[w - 20, tall - 20]}><icon name="triangle-alert" size={12} color="warning" /></box>}
  </box>
);
