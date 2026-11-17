import { h, type Child } from "../sdk";
import theme from "../theme.json";

const [CELL, ROW, GAP] = [theme.size.cell, theme.size.row, 8];

/** How wide a display of `columns` cells is, for what draws in it. */
export const displayWidth = (columns: 4 | 6) => columns * CELL - GAP;
export const DISPLAY_HEIGHT = 2 * ROW;

/**
 * What a device does to the sound, at the left of its card: two rows tall, four columns wide, or
 * six for a file or an EQ. Draw in it with `shapes`; `caption` is the line at its foot that says
 * only what moves.
 */
export const Display = ({ columns = 4, caption, children }: { columns?: 4 | 6; caption?: string; children?: Child }) => (
  <box w={displayWidth(columns)} h={DISPLAY_HEIGHT} radius="md" fill="well" clip>
    {children}
    {caption !== undefined && <text at={[8, DISPLAY_HEIGHT - 22]} style="number" color="text-secondary">{caption}</text>}
  </box>
);
