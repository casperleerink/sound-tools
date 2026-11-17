import { h, type Node } from "../../sdk";
import theme from "../../theme.json";
import { Button } from "../button";

const [ROW, PAD, HEADER] = [theme.size.row, 16, 32];
/** Every card is this tall, so a rack has a straight bottom. */
export const CARD_HEIGHT = HEADER + 2 * ROW + PAD;

/** `cells` as columns of two, top to bottom then left to right. */
const columns = (cells: readonly Node[]) =>
  Array.from({ length: Math.ceil(cells.length / 2) }, (_, i) => <col>{cells.slice(2 * i, 2 * i + 2)}</col>);

/**
 * A device's card: its title, then its `display` at the left and its `cells` in columns of two
 * rows (`Cell`, `Knob`). Cards grow wider, never taller: `more` shows to the right of a hairline
 * while `expanded`, and `onExpand` gives the card its More.
 */
export const DeviceCard = ({ id, title, display, cells = [], more = [], expanded = false, onExpand }: { id: string; title: string; display?: Node; cells?: readonly Node[]; more?: readonly Node[]; expanded?: boolean; onExpand?: () => void }) => (
  <col key={id} h={CARD_HEIGHT} radius="lg" fill="surface" border="border-strong">
    <row h={HEADER} px={4} gap={2} align="center" justify="between">
      <text style="heading" color="text" truncate>{title}</text>
      {more.length > 0 && onExpand && <Button id={`${id}-more`} label={`${expanded ? "Less" : "More"} of ${title}`} text={expanded ? "Less" : "More"} variant="ghost" size="sm" onPress={onExpand} />}
    </row>
    <row px={4} gap={2}>
      {display}
      <row>{columns(cells)}</row>
      {expanded && more.length > 0 && <box w={1} h={2 * ROW} fill="border-strong" />}
      {expanded && <row>{columns(more)}</row>}
    </row>
  </col>
);
