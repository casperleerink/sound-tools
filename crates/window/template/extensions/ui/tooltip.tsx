import { h } from "../sdk";

/** A tip on the element it is in, which needs a `key`: shown while the pointer or the keys are on it. */
export const Tooltip = ({ text, side = "top" }: { text: string; side?: "top" | "bottom" }) => (
  <box overlay={{ open: "hover", side, align: "center", gap: 6 }} role="tooltip" px={2} py={1} radius="sm" fill="raised" border="border-strong" shadow="md">
    <text style="label" color="text">{text}</text>
  </box>
);
