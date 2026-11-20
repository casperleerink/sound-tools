import { h, type Live, type Unit } from "../sdk";
import { Cell } from "./cell";

export type ReadoutProps = { id: string; label: string; text: string; span?: number } & ({ value: string; unit?: never; digits?: never } | { value: Live; unit?: Unit; digits?: number });

/** A value that is no control, in a cell: its name and what it is now, such as the auto gain of a saturator. */
export const Readout = ({ id, label, text, span, value, unit, digits }: ReadoutProps) => (
  <Cell text={text} span={span}>
    <row key={id} label={label} value={value} h={24} align="center">
      {typeof value === "string" ? <text style="number" color="text">{value}</text> : unit ? <text style="number" color="text" live={value} unit={unit} /> : <text style="number" color="text" live={value} digits={digits ?? 1} />}
    </row>
  </Cell>
);
