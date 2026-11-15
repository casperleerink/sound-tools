import { type Color } from "../sdk";

/** A small labelled switch, such as mute: lit with `color` when `on`. */
export const Toggle = ({ id, label, on, color, onPress }: { id: string; label: string; on: boolean; color: Color; onPress: () => void }) => (
  <col key={id} w={20} h={18} radius="sm" align="center" justify="center" fill={on ? color : "border"} onPress={onPress}>
    <text style="xs" color={on ? "on-accent" : "text-muted"}>{label}</text>
  </col>
);
