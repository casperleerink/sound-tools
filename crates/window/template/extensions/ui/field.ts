// A number of a record wired to a control: its range, unit and default from its field, what a
// lane plays while one holds it, the sound following a drag, and its change as one step of undo.
import { live, type FieldInfo, type Fields, type Live, type Unit } from "../sdk";
import type { Range, Scale } from "./value";

/** What a number field says of itself: a tool's `number(...)`, or one of a generic card. */
export type NumberLike = { min: number; max: number; default?: number; integer?: boolean; unit?: Unit | null; label?: string; scale?: Scale };

/** What a Knob, Fader or Slider of one number takes. */
export type Bound = Range & { id: string; path?: string; label: string; value: number; unit?: Unit; initial?: number; held?: Live; onChange: (value: number) => void };

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
    path,
    label: field.label ?? words(name),
    value,
    min: field.min,
    max: field.max,
    scale: field.scale ?? (field.unit === "hz" && field.min > 0 ? "log" : "linear"),
    ...(field.integer ? { step: 1 } : {}),
    ...(field.unit ? { unit: field.unit } : {}),
    ...(field.default === undefined ? {} : { initial: field.default }),
    ...(held(path) ? { held: live.field(path) } : {}),
    onChange,
  };
};

/** The paths of the values of type `V` in the record `S`, as a lane names them: `depth`, `bands[0].gain_db`, `pads.36.pan`, `osc_1.position`. */
export type PathsOf<S, V> = string extends keyof S ? string : { [K in keyof S & string]-?: Inner<NonNullable<S[K]>, V, K> }[keyof S & string];
type Inner<T, V, P extends string> = T extends V
  ? P
  : T extends readonly (infer I)[]
    ? Inner<NonNullable<I>, V, `${P}[${number}]`>
    : T extends object
      ? string extends keyof T
        ? Inner<NonNullable<T[string & keyof T]>, V, `${P}.${string}`>
        : `${P}.${PathsOf<T, V>}`
      : never;

type Changer = { held: (name: string) => boolean; update: (label: string, change: (draft: { state: object }) => void) => void };

/** `bands[0].gain_db` as steps into the record: `bands`, `0`, `gain_db`. */
const stepsOf = (path: string) => path.split(/[.[\]]+/).filter(Boolean);
/** "LFO rate" after "Change", "cutoff" for "Cutoff": an undo step is a sentence. */
const inSentence = (text: string) => (/^.[a-z]/.test(text) ? text.charAt(0).toLowerCase() + text.slice(1) : text);

/** A tool's own fields as the window's facts say them, for a tool whose facts a test leaves out. */
const infosOf = (fields: Fields): FieldInfo[] =>
  Object.entries(fields).flatMap(([name, field]): FieldInfo[] => {
    const label = field.label ?? words(name);
    if (field.kind === "number") return [{ name, label, kind: "number", min: field.min, max: field.max, scale: field.unit === "hz" && field.min > 0 ? "log" : "linear", integer: field.integer ?? false, unit: field.unit ?? null, default: field.default ?? field.min }];
    if (field.kind === "boolean") return [{ name, label, kind: "switch", default: field.default ?? false }];
    if (field.kind === "choice") return [{ name, label, kind: "choice", options: field.options.map((value) => ({ value, text: String(value) })), default: field.default }];
    return [];
  });

/**
 * The controls of the fields of a record: `const field = binder(self, ctx)`, then
 * `<Knob {...field.number("bands[0].gain_db", "Gain")} />`, `<Choice {...field.choice("type")} />`,
 * `<Toggle {...field.toggle("loop")} />`. The fields are those of `self.facts`, or of a tool's own
 * `fields` when given. `text` is what it shows; an agent knows it as the title and `text`, and
 * its undo step says "Change" and `text`. `change` sets several in one step.
 */
export const binder = <S extends object>(self: { state: S; facts: { title: string; fields: readonly FieldInfo[] } }, ctx: Changer, fields?: Fields) => {
  const title = self.facts.title;
  const fieldAt = (path: string): FieldInfo => {
    let found: FieldInfo | undefined;
    let among: readonly FieldInfo[] = fields ? infosOf(fields) : self.facts.fields;
    for (const step of stepsOf(path)) {
      if (found?.kind === "list" || found?.kind === "map") found = found.item;
      else found = among.find((field) => field.name === step);
      among = found?.kind === "group" ? found.fields : [];
    }
    if (!found) throw new Error(`${title} has no field ${path}`);
    return found;
  };
  const valueAt = (path: string): unknown => stepsOf(path).reduce<unknown>((at, step) => (at as Record<string, unknown> | undefined)?.[step], self.state);
  const change = (label: string, values: Record<string, unknown>) =>
    ctx.update(label, (draft) => {
      for (const [path, value] of Object.entries(values)) {
        const steps = stepsOf(path);
        const into = steps.slice(0, -1).reduce<Record<string, unknown>>((at, step) => (at[step] ??= {}) as Record<string, unknown>, draft.state as Record<string, unknown>);
        into[steps.at(-1)!] = value;
      }
    });
  return {
    title,
    change,
    number: (path: PathsOf<S, number>, text?: string): Bound & { text: string } => {
      const field = fieldAt(path);
      if (field.kind !== "number") throw new Error(`${path} is no number`);
      const said = text ?? field.label;
      const value = valueAt(path);
      return { ...control(ctx.held, path, { ...field, label: `${title} ${said}`, unit: field.unit }, typeof value === "number" ? value : field.default, (to) => change(`Change ${inSentence(said)}`, { [path]: to })), text: said };
    },
    choice: (path: PathsOf<S, string | number>, text?: string) => {
      const field = fieldAt(path);
      if (field.kind !== "choice") throw new Error(`${path} is no choice`);
      const said = text ?? field.label;
      return { id: path, label: `${title} ${said}`, text: said, options: field.options, value: (valueAt(path) as string | number | undefined) ?? field.default, onChange: (to: string | number) => change(`Change ${inSentence(said)}`, { [path]: to }) };
    },
    toggle: (path: PathsOf<S, boolean>, text?: string) => {
      const field = fieldAt(path);
      if (field.kind !== "switch") throw new Error(`${path} is no switch`);
      const said = text ?? field.label;
      const on = (valueAt(path) as boolean | undefined) ?? field.default;
      return { id: path, label: `${title} ${said}`, text: said, on, onToggle: () => change(`Turn ${inSentence(said)} ${on ? "off" : "on"}`, { [path]: !on }) };
    },
  };
};
/** The controls of one record, as `binder` makes them. */
export type Binder<S extends object> = ReturnType<typeof binder<S>>;
