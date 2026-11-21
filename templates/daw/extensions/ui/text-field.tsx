import { h } from "../sdk";
import { corner, height, type Size } from "./control";

/**
 * A box to type a name or a number in: Enter or a press elsewhere gives `onCommit` what it says,
 * Escape gives it `null`. With `focus` it takes the keys as it appears, its text chosen: a
 * rename in place shows it on a press and hides it in `onCommit`.
 */
export const TextField = ({ id, label, value, placeholder, focus, w = 160, size = "md", onCommit }: { id: string; label: string; value: string; placeholder?: string; focus?: true; w?: number; size?: Size; onCommit: (text: string | null) => void }) => (
  <row w={w} h={height(size)} pad={[2, 0]} align="center" radius={corner(size)} fill="well" border="border-strong">
    <field key={id} label={label} value={value} placeholder={placeholder} focus={focus} style={size === "sm" ? "label" : "body"} w={w - 16} onCommit={onCommit} />
  </row>
);
