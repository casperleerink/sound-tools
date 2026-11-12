// The parts of a device card: its display and its cells, as the app's own cards have them.

import { Knob, SegmentedControl, Select, Steps, words } from "./components";
import { Button, h, SpaceProps, Space, Toggle } from "./drawing";
import { Fields, Inputs, NumberField } from "./fields";
import { PointerEvents } from "./live";
import { Child, element, UiNode } from "./nodes";
import { Bind } from "./reactivity";
import { Context } from "./tools";

export type DisplayProps = Pick<SpaceProps, keyof PointerEvents | "key" | "drag" | "onDrop" | "cursor" | "children"> & {
  /** The quiet line under it: its numbers, `A 5 ms · D 350 ms`, or its scale. */
  caption?: Bind<string>;
  /** A control above the drawing, such as a `SegmentedControl` of `size="sm"`. */
  control?: Child;
  /** As wide as the display of a card that shows a file or a spectrum. */
  wide?: boolean;
};

/**
 * The display at the left of a card, which shows what the tool does to the sound. Its children
 * are shapes, as a `Space`'s, in a space 1 wide and 1 tall whatever its size in points: `x` 0 is
 * its left and 1 its right, `y` 0 its top and 1 its bottom. It hears the pointer as a `Space`.
 * `<Display caption="Level"><Wave x={0} y={0} width={1} height={1} list={...} color="text" /></Display>`
 */
export const Display = ({ caption, control, wide, ...space }: DisplayProps): UiNode =>
  element("display", { caption, wide }, [control, Space(space)]);

/**
 * The cells of a card, in two rows, filled by column: a `Knob` is a cell, and any other control
 * goes in a `Cell`. A card shows the first columns and keeps the rest behind its expand icon, so
 * the controls that matter most come first.
 */
export const Controls = ({ children }: { children?: Child }): UiNode => element("controls", {}, [children]);

/**
 * A cell of `Controls` for a control that is no knob: a `Select`, a `SegmentedControl`, a
 * `Toggle`, a `NumberBox` or a `Button`, of `size="sm"` as the controls of a card are. `label`
 * and `value` are the lines a knob has under it. It is as many columns wide as its control needs.
 */
export const Cell = ({ children, ...props }: { label?: Bind<string>; value?: Bind<string>; children?: Child }): UiNode =>
  element("cell", props, [children]);

/** The view of a tool that has none: a cell for each field and input, the numbers first. */
export function defaultView(fields: Fields, inputs: Inputs = {}) {
  return ({ state, update, send, openFile }: Context<Record<string, unknown>>): UiNode => {
    const knobs: Child[] = [];
    const cells: Child[] = [];
    // Rows of steps, one over the other, at the left of the cells, where a card has its display.
    const patterns: Child[] = [];
    const knob = (name: string, field: NumberField) =>
      Knob({ field: name, label: field.label ?? words(name), min: field.min, max: field.max, default: field.default ?? field.min, unit: field.unit });
    for (const [name, field] of Object.entries(fields)) {
      const label = field.label ?? words(name);
      if (field.kind === "number") {
        knobs.push(knob(name, field));
      } else if (field.kind === "list" && field.item.kind === "number") {
        patterns.push(Steps({ field: name, label }));
      } else if (field.kind === "file") {
        const choose = async () => {
          const chosen = await openFile({ title: `Choose ${label.toLowerCase()}` });
          if (chosen !== null) update(`Choose ${label.toLowerCase()}`, ({ state: next }) => {
            next[name] = chosen;
          });
        };
        cells.push(Cell({ label, value: () => (state[name] as string) || "No sound", children: Button({ label: "Choose…", variant: "subtle", size: "sm", onClick: choose }) }));
      } else if (field.kind === "boolean") {
        const turn = () => {
          const on = state[name] as boolean;
          update(`Turn ${label.toLowerCase()} ${on ? "off" : "on"}`, ({ state: next }) => {
            next[name] = !on;
          });
        };
        cells.push(Cell({ label, children: Toggle({ dot: true, size: "sm", on: () => state[name] as boolean, onClick: turn }) }));
      } else if (field.kind === "choice") {
        const options = field.options.map((option) => ({ value: option, label: words(String(option)) }));
        // More than a few side by side would make the card wide.
        const control = options.length > 3 ? Select({ field: name, size: "sm", options }) : SegmentedControl({ field: name, size: "sm", options });
        cells.push(Cell({ label, children: control }));
      }
    }
    for (const [name, input] of Object.entries(inputs)) {
      if (input.kind === "number") {
        knobs.push(knob(name, input));
      } else if (input.kind === "bang") {
        cells.push(Cell({ children: Button({ label: input.label ?? words(name), variant: "subtle", size: "sm", onClick: () => (send as (input: string) => void)(name) }) }));
      }
    }
    const controls = Controls({ children: [knobs, cells] });
    if (patterns.length === 0) return controls;
    return h("div", { style: { direction: "row", gap: "lg" } }, h("div", { style: { justify: "center" } }, patterns), controls);
  };
}
