import { h, live } from "../sdk";
import theme from "../theme.json";

/**
 * A drum pad that plays `pitch` on a press and lights green while it sounds, whoever plays it.
 * `chosen` outlines it, as the pad whose sound the card shows.
 */
export const Pad = ({ id, label, text, pitch, path, chosen = false, w = theme.size.cell, h: tall = theme.size.cell, onPress }: { id: string; label: string; text?: string; pitch: number; path?: string; chosen?: boolean; w?: number; h?: number; onPress: () => void }) => (
  <box key={id} label={label} role="button" w={w} h={tall} radius="md" fill="fill" hover="fill" border={chosen ? "text" : "border"} clip onPress={onPress}>
    <box at={[0, 0]} w={w} h={tall} fill="sound/24" opacity={live.sounding(pitch, path)} inert />
    <text at={[8, 6]} style="label" color={chosen ? "text" : "text-secondary"}>{text ?? label}</text>
  </box>
);
