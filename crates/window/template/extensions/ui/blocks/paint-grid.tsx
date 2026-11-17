import { h } from "../../sdk";
import { tint } from "../control";

const CELL = 24;

/**
 * Cells a drag paints, such as a pattern or a game of life: a press, and every cell a held press
 * enters, gets `paint`; Space toggles the cell that has the keys. One stop of Tab.
 */
export const PaintGrid = ({ id, label, cells, paint = true, onPaint, onToggle }: { id: string; label: string; cells: readonly (readonly boolean[])[]; paint?: boolean; onPaint: (row: number, column: number, on: boolean) => void; onToggle: (row: number, column: number) => void }) => (
  <col key={id} label={label} role="grid" group gap={1}>
    {cells.map((row, r) => (
      <row gap={1}>
        {row.map((on, c) => (
          <box key={`${id}-${r}-${c}`} label={`${label} ${r + 1}, ${c + 1}`} role="gridcell" checked={on} w={CELL} h={CELL} radius="sm" fill={on ? tint("on") : "fill"} border={on ? "text" : undefined} hover="fill"
            onEnter={() => onPaint(r, c, paint)} onPress={() => onToggle(r, c)} actions={{ toggle: () => onToggle(r, c) }} />
        ))}
      </row>
    ))}
  </col>
);
