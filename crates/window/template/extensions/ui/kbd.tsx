import { h } from "../sdk";

/** A key as the composer presses it, such as `⌘Z`. */
export const Kbd = ({ keys }: { keys: string }) => (
  <row h={20} pad={[1, 0]} align="center" radius="sm" border="border-strong">
    <text style="number" color="text-secondary">{keys}</text>
  </row>
);
