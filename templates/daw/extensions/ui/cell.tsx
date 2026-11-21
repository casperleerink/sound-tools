import { h, type Child } from "../sdk";
import theme from "../theme.json";

const [CELL, ROW] = [theme.size.cell, theme.size.row];

/**
 * The place of one control on a card, `span` columns wide: the control, then its name and its
 * value, so every row of a card lines up. A cell says each thing once: leave out `text` for a
 * switch, which carries its name, and `value` for a control that shows its own.
 */
export const Cell = ({ text, value, span = 1, children }: { text?: string; value?: string; span?: number; children?: Child }) => (
  <col w={span * CELL} h={ROW} align="center">
    <row w="full" h={theme.size.knob} align="center" justify="center">{children}</row>
    <box h={2} />
    {text !== undefined && <text style="label" color="text-secondary">{text}</text>}
    {value !== undefined && <text style="number" color="text">{value}</text>}
  </col>
);
