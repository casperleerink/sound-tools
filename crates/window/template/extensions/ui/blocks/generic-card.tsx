import { h, type Card, type FieldInfo, type Node } from "../../sdk";
import { control as bound } from "../field";
import theme from "../../theme.json";
import { Cell } from "../cell";
import { Knob } from "../knob";
import { Segmented } from "../segmented";
import { Select } from "../select";
import { Toggle } from "../toggle";
import { DeviceCard } from "./device-card";

/**
 * A field as the facts of the window give it. They will also carry a number's `scale` in place of
 * `log`, and records inside records: a group of fields, a list of items (the bands of an EQ) and a
 * map of items by key (the pads of a drum pad), as `sound_core::facts` makes them.
 */
type Field =
  | Exclude<FieldInfo, { kind: "number" }>
  | (Omit<Extract<FieldInfo, { kind: "number" }>, "log"> & { log?: boolean; scale?: "linear" | "logarithmic" | "fader" })
  | { name: string; label: string; kind: "group"; fields: readonly Field[] }
  | { name: string; label: string; kind: "list"; item: Field; max_items?: number | null }
  | { name: string; label: string; kind: "map"; item: Field };
type Path = readonly (string | number)[];
/** A control in its place: how many columns it takes, and where it sits among the others. */
type Placed = { span: number; rank: number; node: Node };

/** How many cells show until the card is expanded (three columns of two); the most options side by side, and their letters, that fit two columns. */
const [FEW, SIDE_BY_SIDE, LETTERS] = [6, 3, 14];
/** Knobs and switches two to a column, then choices, two columns wide. */
const RANK = { number: 0, switch: 1, choice: 2 };

/**
 * The card of every device whose tool has no view and no card of its own: a knob for a number, a
 * switch for a yes or no, a choice of options, and for a list or a map of records, a choice of
 * which item its controls change. The first few in the order of the record show, the rest behind
 * More.
 */
export const card: Card = ({ state, facts }, { update, memory, held }) => {
  const title = facts.title;
  const at = (path: Path): unknown => path.reduce<unknown>((value, step) => (value as Record<string | number, unknown> | undefined)?.[step], state);
  const set = (path: Path, label: string, value: unknown) =>
    update(`Change ${label.toLowerCase()}`, (draft) => {
      let into = draft.state as Record<string | number, unknown>;
      for (const step of path.slice(0, -1)) into = (into[step] ??= {}) as Record<string | number, unknown>;
      into[path.at(-1)!] = value;
    });
  /** The item of the list or map at `path` its controls change, by its card's memory. */
  const chosen = memory as Record<string, string | number | undefined>;

  /** The control of a field `said` as `label` (for undo and an agent), at `path` of the record. */
  const one = (field: Field, path: Path, label: string): Placed[] => {
    const id = path.join(".");
    const named = `${title} ${label}`;
    switch (field.kind) {
      case "number": {
        const value = typeof at(path) === "number" ? (at(path) as number) : field.default;
        const log = field.log ?? field.scale === "logarithmic";
        const knob = bound(held, id, { ...field, log }, value, (to) => set(path, label, to));
        return [{ span: 1, rank: RANK.number, node: <Knob {...knob} bipolar={field.min < 0 && field.min === -field.max} label={named} text={field.label} /> }];
      }
      case "switch": {
        const on = at(path) === undefined ? field.default : at(path) === true;
        // A switch says its name: a long one takes two columns.
        const span = field.label.length > LETTERS / 2 ? 2 : 1;
        return [{ span, rank: RANK.switch, node: <Cell span={span}><Toggle id={id} label={named} text={field.label} on={on} w={span * theme.size.cell - 8} onToggle={() => set(path, label, !on)} /></Cell> }];
      }
      case "choice": {
        const value = (at(path) as string | number | undefined) ?? field.default;
        const choose = (to: string | number) => set(path, label, to);
        const few = field.options.length <= SIDE_BY_SIDE && field.options.reduce((sum, option) => sum + option.text.length, 0) <= LETTERS;
        const node = few
          ? <Cell text={field.label} span={2}><Segmented id={id} label={named} options={field.options} value={value} onChange={choose} /></Cell>
          : <Cell text={field.label} span={2}><Select id={id} label={named} options={field.options} value={value} w={2 * theme.size.cell - 8} search={field.options.length > 8} onChange={choose} /></Cell>;
        return [{ span: 2, rank: RANK.choice, node }];
      }
      case "group":
        return field.fields.flatMap((inner) => one(inner, [...path, inner.name], `${label} ${inner.label.toLowerCase()}`));
      case "list":
      case "map": {
        const value = at(path);
        const keys: (string | number)[] = field.kind === "list" ? (Array.isArray(value) ? value.map((_, i) => i) : []) : Object.keys(value ?? {});
        if (keys.length === 0) return [];
        const item = keys.includes(chosen[id] ?? keys[0]!) ? (chosen[id] ?? keys[0]!) : keys[0]!;
        const options = keys.map((key) => ({ value: key, text: typeof key === "number" ? String(key + 1) : key }));
        const pick = (to: string | number) => (chosen[id] = to);
        const node = options.length <= 4
          ? <Cell text={field.label} span={2}><Segmented id={id} label={named} options={options} value={item} onChange={pick} /></Cell>
          : <Cell text={field.label} span={2}><Select id={id} label={named} options={options} value={item} w={2 * theme.size.cell - 8} onChange={pick} /></Cell>;
        const which = typeof item === "number" ? `${field.label} ${item + 1}` : `${field.label} ${item}`;
        const inner = field.item.kind === "group" ? field.item.fields.flatMap((each) => one(each, [...path, item, each.name], `${which} ${each.label.toLowerCase()}`)) : one({ ...field.item, label: which }, [...path, item], which);
        return [{ span: 2, rank: RANK.choice, node }, ...inner];
      }
    }
  };

  const fields = facts.fields as readonly Field[];
  const plain = fields.filter((field): field is Exclude<Field, { kind: "group" | "list" | "map" }> => field.kind === "number" || field.kind === "switch" || field.kind === "choice");
  const blocks = fields.filter((field) => !plain.some((each) => each === field)).map((field) => one(field, [field.name], field.label)).filter((block) => block.length > 0);
  // The record's order decides what shows first; a kind only where it sits. A list or a group shows whole, or behind More.
  const [shown, hidden]: Placed[][] = [[], []];
  let room = FEW;
  for (const placed of plain.flatMap((field) => one(field, [field.name], field.label))) {
    const fits = hidden.length === 0 && placed.span <= room;
    (fits ? shown : hidden).push(placed);
    if (fits) room -= placed.span;
  }
  const byRank = (some: Placed[]) => [...some].sort((a, b) => a.rank - b.rank);
  const [cells, more] = [byRank(shown), byRank(hidden)];
  for (const block of blocks) {
    // A card of lists alone shows the first of them as far as it fits.
    const split = cells.length === 0 ? block.findIndex((placed, i) => block.slice(0, i + 1).reduce((sum, each) => sum + each.span, 0) > FEW) : 0;
    const cut = split < 0 ? block.length : split;
    cells.push(...block.slice(0, cut));
    more.push(...block.slice(cut));
  }
  return <DeviceCard id="card" title={title} cells={cells.map((placed) => placed.node)} more={more.map((placed) => placed.node)} expanded={memory.expanded === true} onExpand={() => (memory.expanded = memory.expanded !== true)} />;
};
