import { h, type Child } from "../../sdk";

/** What a place says while it has nothing to show, and the one thing to do about it. */
export const EmptyState = ({ title, text, children }: { title: string; text?: string; children?: Child }) => (
  <col w="full" pad={8} gap={2} align="center" justify="center">
    <text style="heading" color="text">{title}</text>
    {text !== undefined && <text style="body" color="text-secondary">{text}</text>}
    {children}
  </col>
);
