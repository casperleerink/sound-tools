import { type Color } from "../sdk";

/** A square button with a glyph: lit with `color` when `on`. */
export const Button = ({ id, glyph, on, color = "accent", onPress }: { id: string; glyph: string; on: boolean; color?: Color; onPress: () => void }) => (
  <col key={id} w={28} h={28} radius="md" align="center" justify="center" fill={on ? color : "border"} onPress={onPress}>
    <text style="sm" color={on ? "on-accent" : "text"}>{glyph}</text>
  </col>
);
