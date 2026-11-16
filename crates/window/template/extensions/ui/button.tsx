import { h, type Color } from "../sdk";

/** A button with a glyph or a word, lit with `color` while `on`. */
export const Button = ({ id, label, glyph, on = false, color = "green", onPress }: { id: string; label: string; glyph: string; on?: boolean; color?: Color; onPress: () => void }) => (
  <row key={id} label={label} role="button" h={28} px={3} radius="md" align="center" justify="center" fill={on ? color : "fill"} onPress={onPress}>
    <text style="label" color={on ? "text-inverse" : "text"}>{glyph}</text>
  </row>
);
