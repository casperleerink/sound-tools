// The parts of a device card, as the app's own cards have them. A view gives its display, or
// rows of `Steps` in its place, and then its controls, and the window lays the card out:
// `<><Display>...</Display><Controls>...</Controls></>`. A `Panel` of a page takes the same parts
// in a `Row`.

import { Knob, SegmentedControl, Select, Steps, words } from "./components";
import { Button, SpaceProps, Space, Toggle } from "./drawing";
import { Fields, Inputs, isPattern } from "./fields";
import { PointerEvents } from "./live";
import { Child, element, UiNode } from "./nodes";
import { Bind } from "./reactivity";
import { Context } from "./tools";

/**
 * The marks of a display's scale, faint behind its drawing as the app's own displays have them,
 * at places from 0 to 1 as its shapes have them: a line up and down at each `x`, with a word at
 * its top where it has one, `[0.5, "1k"]`, and a line across at each `y`.
 */
export interface DisplayScale {
  x?: readonly (number | readonly [at: number, word: string])[];
  y?: readonly number[];
}

export type DisplayProps = Pick<SpaceProps, keyof PointerEvents | "key" | "drag" | "onDrop" | "cursor" | "children"> & {
  scale?: Bind<DisplayScale>;
  /** The line of no change, a little brighter than a mark, at a place down from 0 to 1: 0.5 for a wave. */
  zero?: Bind<number>;
  /** The quiet line under it, for what no mark and no control says: its numbers, `A 5 ms · D 350 ms`. */
  caption?: Bind<string>;
  /** A control above the drawing, such as a `SegmentedControl`. */
  control?: Child;
  /** As wide as the display of a card that shows a file or a spectrum. */
  wide?: boolean;
};

/**
 * The display at the left of a card, which shows what the tool does to the sound. Its children
 * are shapes, as a `Space`'s, in a space 1 wide and 1 tall whatever its size in points: `x` 0 is
 * its left and 1 its right, `y` 0 its top and 1 its bottom, a little inside its edges. It hears
 * the pointer as a `Space`.
 * `<Display zero={0.5}><Wave list={now.scope()} /></Display>`
 */
export const Display = ({ scale, zero, caption, control, wide, ...space }: DisplayProps): UiNode =>
  element("display", { scale, zero, caption, wide }, [control, Space(space)]);

/**
 * The one home of the controls of a card or a panel: its cells, in two rows, filled by column.
 * A control on a `field` is a cell by itself, with the field's label under it: a `Knob`, a
 * `SegmentedControl`, a `Select`, a `NumberBox`, a `Slider`, and a `Toggle`, which says its name
 * itself. A card shows the first columns, two beside a display or rows of steps, and keeps the
 * rest behind its expand icon, so the controls that matter most come first; a page shows them
 * all. A knob with its label anywhere else, in a `Row` or straight in a `Panel` or a card, is a
 * problem of the view.
 */
export const Controls = ({ children }: { children?: Child }): UiNode => element("controls", {}, [children]);

/**
 * A cell of `Controls` for what has no field: a `Button`, or a control on what code keeps. A
 * control in a cell is the size of a card's by itself; a `Select` and a `NumberBox` still say
 * `size="sm"`. `label` and `value` are the lines a knob has under it; `value` reads what the
 * control does not say itself, and never a unit alone. It is as many columns wide as its
 * control needs.
 */
export const Cell = ({ children, ...props }: { label?: Bind<string>; value?: Bind<string>; children?: Child }): UiNode =>
  element("cell", props, [children]);

/** The view of a tool that has none: a row of steps for each `pattern`, and a cell for each other field and input, the numbers first. */
export function defaultView(fields: Fields, inputs: Inputs = {}) {
  return ({ state, update, send, openFile }: Context<Record<string, unknown>>): Child => {
    const knobs: Child[] = [];
    const cells: Child[] = [];
    const patterns: Child[] = [];
    for (const [name, field] of Object.entries(fields)) {
      const label = field.label ?? words(name);
      if (field.kind === "number") {
        knobs.push(Knob({ field: name }));
      } else if (isPattern(field)) {
        patterns.push(Steps({ field: name, label }));
      } else if (field.kind === "file") {
        const choose = async () => {
          const chosen = await openFile({ title: `Choose ${label.toLowerCase()}` });
          if (chosen !== null) update(`Choose ${label.toLowerCase()}`, ({ state: next }) => {
            next[name] = chosen;
          });
        };
        cells.push(Cell({ label, value: () => (state[name] as string) || "No sound", children: Button({ label: "Choose…", variant: "subtle", onClick: choose }) }));
      } else if (field.kind === "boolean") {
        cells.push(Toggle({ field: name }));
      } else if (field.kind === "choice") {
        // More than a few side by side would make the card wide.
        cells.push(field.options.length > 3 ? Select({ field: name, size: "sm" }) : SegmentedControl({ field: name }));
      }
    }
    for (const [name, input] of Object.entries(inputs)) {
      if (input.kind === "number") {
        knobs.push(Knob({ field: name }));
      } else if (input.kind === "bang") {
        cells.push(Cell({ children: Button({ label: input.label ?? words(name), variant: "subtle", onClick: () => (send as (input: string) => void)(name) }) }));
      }
    }
    return [patterns, Controls({ children: [knobs, cells] })];
  };
}
