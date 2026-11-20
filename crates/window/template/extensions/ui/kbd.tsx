import { h } from "../sdk";

/** A key as the composer presses it, such as `⌘Z`. */
export const Kbd = ({ keys }: { keys: string }) => (
  <row h={20} pad={[1, 0]} align="center" radius="sm" border="border-strong">
    <text style="number" color="text-secondary">{keys}</text>
  </row>
);

const SIGNS: Record<string, string> = {
  cmd: "⌘", shift: "⇧", alt: "⌥", ctrl: "⌃", enter: "⏎", backspace: "⌫", delete: "⌦", escape: "Esc", space: "Space", tab: "⇥",
  up: "↑", down: "↓", left: "←", right: "→", pageup: "PgUp", pagedown: "PgDn", home: "Home", end: "End",
};
/** The order a Mac draws held keys in, as the app's menus do. */
const HELD = ["ctrl", "alt", "shift", "cmd"];
/** A key as an action names it, `"cmd-shift-z"` or `"cmd--"`, as the composer reads it, `⇧⌘Z` or `⌘-`; none for none. */
export const keysOf = (key: string | null): string | undefined => {
  if (key === null) return undefined;
  const parts = key.split(/-(?!$)/);
  const held = parts.slice(0, -1).sort((a, b) => HELD.indexOf(a) - HELD.indexOf(b));
  return [...held, ...parts.slice(-1)].map((part) => SIGNS[part] ?? part.toUpperCase()).join("");
};
