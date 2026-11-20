import { h } from "../sdk";

/** A key as the composer presses it, such as `⌘Z`. */
export const Kbd = ({ keys }: { keys: string }) => (
  <row h={20} px={1} align="center" radius="sm" border="border-strong">
    <text style="number" color="text-secondary">{keys}</text>
  </row>
);

const SIGNS: Record<string, string> = {
  cmd: "⌘", shift: "⇧", alt: "⌥", ctrl: "⌃", enter: "⏎", backspace: "⌫", delete: "⌦", escape: "Esc", space: "Space", tab: "⇥",
  up: "↑", down: "↓", left: "←", right: "→", pageup: "PgUp", pagedown: "PgDn", home: "Home", end: "End",
};
/** A key as an action names it, `"cmd-shift-z"` or `"cmd--"`, as the composer reads it, `⌘⇧Z` or `⌘-`; none for none. */
export const keysOf = (key: string | null): string | undefined =>
  key === null ? undefined : key.split(/-(?!$)/).map((part) => SIGNS[part] ?? part.toUpperCase()).join("");
