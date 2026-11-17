import { h, live, type GenericCard } from "../../sdk";
import theme from "../../theme.json";
import { Cell } from "../cell";
import { Knob } from "../knob";
import { Segmented } from "../segmented";
import { Select } from "../select";
import { Toggle } from "../toggle";
import { DeviceCard } from "./device-card";

type Field = Parameters<GenericCard>[0]["card"]["fields"][number];

/** How many controls a card shows until it is expanded, and the most options side by side. */
const [FEW, SIDE_BY_SIDE] = [4, 3];
/** Within what shows, knobs and switches two to a column, then choices, two columns wide. */
const RANK = { number: 0, switch: 1, choice: 2 };

/**
 * The card of every device whose tool has no view, such as a built-in one: a knob for a number,
 * a switch for a yes or no, a choice of options: the first few of its record shown, the rest
 * behind More.
 */
export const card: GenericCard = ({ state, card: { title, fields } }, { update, memory, held }) => {
  const set = (field: Field, value: unknown) => update(`Change ${field.label}`, (draft) => void (draft.state[field.name] = value));
  const control = (field: Field) => {
    const label = `${title} ${field.label}`;
    switch (field.kind) {
      case "number": {
        const value = typeof state[field.name] === "number" ? (state[field.name] as number) : field.default;
        return <Knob id={field.name} label={label} text={field.label} value={value} min={field.min} max={field.max} scale={field.log ? "log" : "linear"} step={field.integer ? 1 : undefined} unit={field.unit ?? undefined} initial={field.default} held={held(field.name) ? live.field(field.name) : undefined} onChange={(to) => set(field, to)} />;
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
  // The tool's order decides what shows first; its kind only where it sits.
  const laid = (some: Field[]) => [...some].sort((a, b) => RANK[a.kind] - RANK[b.kind]).map(control);
  return (
    <DeviceCard id="card" title={title} cells={laid(fields.slice(0, FEW))} more={laid(fields.slice(FEW))} expanded={memory.expanded === true} onExpand={() => (memory.expanded = memory.expanded !== true)} />
  );
};
