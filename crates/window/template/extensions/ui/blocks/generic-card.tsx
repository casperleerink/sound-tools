import { h, type Card, type FieldInfo } from "../../sdk";
import { control as bound } from "../field";
import theme from "../../theme.json";
import { Cell } from "../cell";
import { Knob } from "../knob";
import { Segmented } from "../segmented";
import { Select } from "../select";
import { Toggle } from "../toggle";
import { DeviceCard } from "./device-card";

/** How many controls a card shows until it is expanded, and the most options side by side. */
const [FEW, SIDE_BY_SIDE] = [4, 3];
/** Within what shows, knobs and switches two to a column, then choices, two columns wide. */
const RANK = { number: 0, switch: 1, choice: 2 };

type Value = FieldInfo & { kind: keyof typeof RANK };
/** A value of the record a control sets: where it is, as `bands.0.gain_db`, and what it says. */
type Leaf = { path: (string | number)[]; label: string; field: Value };

const read = (state: unknown, path: readonly (string | number)[]): unknown =>
  path.reduce<unknown>((inside, step) => (inside as Record<string | number, unknown> | undefined)?.[step], state);

/**
 * The values of `fields` in `state`, each with its path: the fields of a group, and those of
 * each item of a list or a map the record has now, labelled with the group, item or key.
 */
const leaves = (fields: readonly FieldInfo[], state: unknown, path: (string | number)[] = [], said = ""): Leaf[] =>
  fields.flatMap((field) => {
    const at = field.name === "" ? path : [...path, field.name];
    const label = [said, field.label].filter(Boolean).join(" ");
    switch (field.kind) {
      case "group":
        return leaves(field.fields, state, at, label);
      case "list": {
        const items = read(state, at);
        const count = Array.isArray(items) ? items.length : 0;
        // One of `Bands` is `Band 1`.
        const one = label.replace(/s$/, "");
        return Array.from({ length: count }, (_, i) => leaves([field.item], state, [...at, i], `${one} ${i + 1}`)).flat();
      }
      case "map": {
        const items = read(state, at);
        const keys = items && typeof items === "object" ? Object.keys(items) : [];
        return keys.flatMap((key) => leaves([field.item], state, [...at, key], `${label} ${key}`));
      }
      default:
        return [{ path: at, label, field }];
    }
  });

/**
 * The card of every device whose tool has no view, such as a built-in one: a knob for a number,
 * a switch for a yes or no, a choice of options, for every value of its record, also those in
 * groups and lists: the first few shown, the rest behind More.
 */
export const card: Card = ({ state, facts: { title, fields } }, { update, memory, held }) => {
  const set = ({ path, label }: Leaf, value: unknown) =>
    update(`Change ${label}`, (draft) => {
      const inside = read(draft.state, path.slice(0, -1)) as Record<string | number, unknown>;
      inside[path.at(-1)!] = value;
    });
  const control = (leaf: Leaf) => {
    const { field, label } = leaf;
    const [id, value] = [leaf.path.join("."), read(state, leaf.path)];
    const named = `${title} ${label}`;
    switch (field.kind) {
      case "number": {
        const now = typeof value === "number" ? value : field.default;
        return <Knob {...bound(held, id, field, now, (to) => set(leaf, to))} label={named} text={label} />;
      }
      case "switch":
        return <Cell><Toggle id={id} label={named} text={label} on={(value ?? field.default) === true} w={theme.size.cell - 8} onToggle={() => set(leaf, (value ?? field.default) !== true)} /></Cell>;
      case "choice": {
        const now = (value as string | number | undefined) ?? field.default;
        const choose = (to: string | number) => set(leaf, to);
        return field.options.length <= SIDE_BY_SIDE
          ? <Cell text={label} span={2}><Segmented id={id} label={named} options={field.options} value={now} onChange={choose} /></Cell>
          : <Cell text={label} span={2}><Select id={id} label={named} options={field.options} value={now} w={2 * theme.size.cell - 8} onChange={choose} /></Cell>;
      }
    }
  };
  const all = leaves(fields, state);
  // The tool's order decides what shows first; its kind only where it sits.
  const laid = (some: Leaf[]) => [...some].sort((a, b) => RANK[a.field.kind] - RANK[b.field.kind]).map(control);
  return (
    <DeviceCard id="card" title={title} cells={laid(all.slice(0, FEW))} more={laid(all.slice(FEW))} expanded={memory.expanded === true} onExpand={() => (memory.expanded = memory.expanded !== true)} />
  );
};
