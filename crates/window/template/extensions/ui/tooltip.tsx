import { h } from "../sdk";
import { Kbd } from "./kbd";

/** A tip on the element it is in, which needs a `key`: shown while the pointer or the keys are on it, with the key that does the same. */
export const Tooltip = ({ text, keys, side = "top" }: { text: string; keys?: string; side?: "top" | "bottom" }) => (
  <row overlay={{ open: "hover", side, align: "center", gap: 6 }} role="tooltip" pad={[2, 1]} gap={2} align="center" radius="sm" fill="raised" border="border-strong" shadow="md">
    <text style="label" color="text">{text}</text>
    {keys !== undefined && <Kbd keys={keys} />}
  </row>
);
