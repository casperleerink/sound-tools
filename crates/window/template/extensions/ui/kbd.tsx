import { h } from "../sdk";

/** A key as the composer presses it, such as `⌘Z`. */
export const Kbd = ({ keys }: { keys: string }) => (
  <row h={20} px={1} align="center" radius="sm" border="border-strong">
    <text style="number" color="text-secondary">{keys}</text>
  </row>
);

const SIGNS: Record<string, string> = { cmd: "⌘", alt: "⌥", shift: "⇧", ctrl: "⌃", left: "←", right: "→", up: "↑", down: "↓", enter: "↩", escape: "⎋", backspace: "⌫", delete: "⌦", space: "Space" };
/** A key as a tool names it, `alt-left`, as a Mac shows it: `⌥←`. */
export const keysOf = (shortcut: string) => shortcut.split("-").map((part) => SIGNS[part] ?? part.toUpperCase()).join("");
