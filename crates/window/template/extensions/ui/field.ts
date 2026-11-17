// A number of a record wired to a control: its range, unit and default from its field, what a
// lane plays while one holds it, and its change as one step of undo. `<Knob {...field("depth")} />`.
import { live, type Fields, type Live, type Unit } from "../sdk";
import type { Range } from "./value";

/** What a number field says of itself: a tool's `number(...)`, or one of a generic card. */
export type NumberLike = { min: number; max: number; default?: number; integer?: boolean; unit?: Unit | null; label?: string; log?: boolean };

/** What a Knob, Fader or Slider of one number takes. */
export type Bound = Range & { id: string; label: string; value: number; unit?: Unit; initial?: number; held?: Live; onChange: (value: number) => void };

/** `cutoff_hz` as `Cutoff`: a field's name in words, without its unit. */
const words = (name: string) => {
  const parts = name.split("_");
  if (parts.length > 1 && ["hz", "db", "ms", "s", "seconds", "pct", "semitones", "cents"].includes(parts.at(-1)!)) parts.pop();
  const said = parts.join(" ");
  return said.charAt(0).toUpperCase() + said.slice(1);
};

/**
 * The control of the number `path` (`cutoff_hz` of the tool's own record, or `t00/mix.gain_db`
 * of one inside), whose field is `field`, now at `value`: `held` while a lane moves it.
 */
export const control = (held: (name: string) => boolean, path: string, field: NumberLike, value: number, onChange: (value: number) => void): Bound => {
  const name = path.slice(path.lastIndexOf(".") + 1);
  return {
    id: path,
    label: field.label ?? words(name),
    value,
    min: field.min,
    max: field.max,
    scale: (field.log ?? (field.unit === "hz" && field.min > 0)) ? "log" : "linear",
    ...(field.integer ? { step: 1 } : {}),
    ...(field.unit ? { unit: field.unit } : {}),
    ...(field.default === undefined ? {} : { initial: field.default }),
    ...(held(path) ? { held: live.field(path) } : {}),
    onChange,
  };
};

/** The control of each number field of a tool's own record: `binder(self, ctx, fields)("depth")`. */
export const binder = (self: { state: object }, ctx: { held: (name: string) => boolean; update: (label: string, change: (draft: { state: object }) => void) => void }, fields: Fields) =>
  (name: string): Bound => {
    const field = fields[name];
    if (field?.kind !== "number") throw new Error(`${name} is no number field`);
    const label = field.label ?? words(name);
    const value = (self.state as Record<string, number>)[name] ?? field.default ?? field.min;
    return control(ctx.held, name, field, value, (to) => ctx.update(`Change ${label}`, (draft) => void ((draft.state as Record<string, number>)[name] = to)));
  };
