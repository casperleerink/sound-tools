import { h } from "../sdk";
import { corner, height, type Size } from "./control";

/** A box to type a name or a number in: Enter keeps it, Escape or a press elsewhere leaves it. */
export const TextField = ({ id, label, value, placeholder, w = 160, size = "md", onCommit }: { id: string; label: string; value: string; placeholder?: string; w?: number; size?: Size; onCommit: (text: string) => void }) => (
  <row w={w} h={height(size)} px={2} align="center" radius={corner(size)} fill="well" border="border-strong">
    <field key={id} label={label} value={value} placeholder={placeholder} style={size === "sm" ? "label" : "body"} w={w - 16} onCommit={onCommit} />
  </row>
);
