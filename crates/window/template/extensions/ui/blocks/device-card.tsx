import { h, type Node } from "../../sdk";
import theme from "../../theme.json";
import { Toggle } from "../toggle";

const [ROW, PAD, HEADER] = [theme.size.row, 16, 32];
/** Every card is this tall, so a rack has a straight bottom. */
export const CARD_HEIGHT = HEADER + 2 * ROW + theme.space[1] + PAD;

/** How many columns a cell takes. */
const widthOf = (cell: Node) => ("w" in cell && typeof cell.w === "number" ? cell.w : 0);

/**
 * `cells` in two rows, read left to right: each run of cells of one width fills its top row,
 * then its bottom row, so a cell never sits over one of another width.
 */
const columns = (cells: readonly Node[]) => {
  const runs: Node[][] = [];
  for (const cell of cells) {
    const last = runs.at(-1);
    if (last && widthOf(last[0]!) === widthOf(cell)) last.push(cell);
    else runs.push([cell]);
  }
  return runs.flatMap((run) => {
    const across = Math.ceil(run.length / 2);
    return Array.from({ length: across }, (_, i) => <col gap={1}>{[run[i], run[i + across]].filter((cell) => cell !== undefined)}</col>);
  });
};

/**
 * A device's card: its title, then its `display` at the left and its `cells` in columns of two
 * rows (`Cell`, `Knob`). Cards grow wider, never taller: `more` shows to the right of a hairline
 * while `expanded`, and `onExpand` gives the card its More.
 */
export const DeviceCard = ({ id, title, display, cells = [], more = [], expanded = false, onExpand }: { id: string; title: string; display?: Node; cells?: readonly Node[]; more?: readonly Node[]; expanded?: boolean; onExpand?: () => void }) => (
  <col key={id} h={CARD_HEIGHT} radius="lg" fill="surface" border="border-strong">
    <row h={HEADER} px={4} gap={4} align="center" justify="between">
      <text style="heading" color="text" truncate>{title}</text>
      {more.length > 0 && onExpand && <Toggle id={`${id}-more`} label={`More of ${title}`} text="More" on={expanded} onToggle={onExpand} />}
    </row>
    <row px={4} gap={2}>
      {display}
      <row>{columns(cells)}</row>
      {expanded && more.length > 0 && <box w={1} h={2 * ROW} fill="border-strong" />}
      {expanded && <row>{columns(more)}</row>}
    </row>
  </col>
);
