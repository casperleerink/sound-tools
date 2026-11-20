import { h } from "../sdk";

/** A key as the composer presses it, such as `⌘Z`. */
export const Kbd = ({ keys }: { keys: string }) => (
  <row h={20} px={1} align="center" radius="sm" border="border-strong">
    <text style="number" color="text-secondary">{keys}</text>
  </row>
);

const SIGNS: Record<string, string> = {
  cmd: "⌘", alt: "⌥", shift: "⇧", ctrl: "⌃", enter: "⏎", escape: "Esc", backspace: "⌫", delete: "⌦", space: "Space", tab: "⇥",
  left: "←", right: "→", up: "↑", down: "↓", pageup: "PgUp", pagedown: "PgDn", home: "Home", end: "End",
};
/** A key as the window names it, `alt-shift-right`, as the composer reads it: `⌥⇧→`. */
export const keyText = (key: string) =>
  key.split(/-(?!$)/).map((part) => SIGNS[part] ?? part.toUpperCase()).join("");
