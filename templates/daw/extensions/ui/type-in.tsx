import { h, type Child, type Drag, type Pointer, type Unit } from "../sdk";
import { shown, typedOver, type Range } from "./value";

/**
 * The value line of a control, which a click types a value into, as Enter on the control does:
 * Enter or a press elsewhere keeps it, Escape leaves it, and text that says what was shown
 * changes nothing. `children` is what it shows; `w` × `h` is what the field covers, from the
 * line's bottom up. `drag` and `onDragEnd` let a drag from the line turn the control.
 */
export const TypeIn = ({ id, label, value, unit, range, w, h: tall, drag, onDragEnd, onChange, children }: {
  id: string; label: string; value: number; unit?: Unit; range: Range; w: number; h: number;
  drag?: Drag; onDragEnd?: (p: Pointer) => void;
  onChange: (value: number) => void; children?: Child;
}) => (
  <row key={`${id}-value`} label={`Type ${label}`} role="button" w={w} justify="center" drag={drag} onDragEnd={onDragEnd}>
    {children}
    <row overlay={{ open: "press", side: "bottom", align: "center", gap: -tall }} w={w} h={tall} pad={[1, 0]} align="center" radius="md" fill="raised" border="focus" shadow="md">
      <field key={`${id}-typed`} label={`${label} value`} value={shown(value, unit)} style="number" align="center" w={w - 8} focus onCommit={(text) => {
        if (text === null) return;
        const typed = typedOver(text, value, range, unit);
        if (typed !== undefined && typed !== value) onChange(typed);
      }} />
    </row>
  </row>
);
