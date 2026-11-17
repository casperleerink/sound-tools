import { h, type CardField, type GenericCard } from "../../sdk";
import theme from "../../theme.json";
import { Cell } from "../cell";
import { Knob } from "../knob";
import { Segmented } from "../segmented";
import { Select } from "../select";
import { Toggle } from "../toggle";
import { DeviceCard } from "./device-card";

/** How many controls a card shows until it is expanded, and the most options side by side. */
const [FEW, SIDE_BY_SIDE] = [4, 3];

/**
 * The card of every device whose tool has no view, such as a built-in one: a knob for a number,
 * a switch for a yes or no, a choice of options, the first few shown and the rest behind More.
 */
export const card: GenericCard = ({ state, card: { title, fields } }, { update, memory }) => {
  const set = (field: CardField, value: unknown) => update(`Change ${field.label}`, (draft) => void (draft.state[field.name] = value));
  const control = (field: CardField) => {
    const label = `${title} ${field.label}`;
    switch (field.kind) {
      case "number": {
        const value = typeof state[field.name] === "number" ? (state[field.name] as number) : field.default;
        return <Knob id={field.name} label={label} text={field.label} value={value} min={field.min} max={field.max} scale={field.log ? "log" : "linear"} step={field.integer ? 1 : undefined} unit={field.unit ?? undefined} initial={field.default} onChange={(to) => set(field, to)} />;
      }
      case "switch":
        return <Cell><Toggle id={field.name} label={label} text={field.label} on={state[field.name] === true} w={theme.size.cell - 8} onToggle={() => set(field, state[field.name] !== true)} /></Cell>;
      case "choice": {
        const value = (state[field.name] as string | number | undefined) ?? field.default;
        const choose = (to: string | number) => set(field, to);
        return field.options.length <= SIDE_BY_SIDE
          ? <Cell text={field.label} span={2}><Segmented id={field.name} label={label} options={field.options} value={value} onChange={choose} /></Cell>
          : <Cell text={field.label} span={2}><Select id={field.name} label={label} options={field.options} value={value} w={2 * theme.size.cell - 8} onChange={choose} /></Cell>;
      }
    }
  };
  // Knobs and switches go two to a column, and choices, two columns wide, beside them.
  const rank = { number: 0, switch: 1, choice: 2 };
  const cells = [...fields].sort((a, b) => rank[a.kind] - rank[b.kind]).map(control);
  return (
    <DeviceCard id="card" title={title} cells={cells.slice(0, FEW)} more={cells.slice(FEW)} expanded={memory.expanded === true} onExpand={() => (memory.expanded = memory.expanded !== true)} />
  );
};
