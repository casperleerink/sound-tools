import { h } from "../sdk";

const SYMBOLS: Record<string, string> = {
  cmd: "⌘", shift: "⇧", alt: "⌥", ctrl: "⌃", enter: "⏎", backspace: "⌫", delete: "⌦", escape: "Esc", space: "Space", tab: "⇥",
  up: "↑", down: "↓", left: "←", right: "→",
};
/** A key as an action names it, `shift-enter`, as the composer reads it: `⇧⏎`. */
export const keysOf = (key: string) => key.split("-").map((part) => SYMBOLS[part] ?? part.toUpperCase()).join("");

/** A key as the composer presses it, such as `⌘Z`. */
export const Kbd = ({ keys }: { keys: string }) => (
  <row h={20} px={1} align="center" radius="sm" border="border-strong">
    <text style="number" color="text-secondary">{keys}</text>
  </row>
);
