import { h, live, type Card, type Node } from "../../../sdk";
import theme from "../../../theme.json";
import { Button } from "../../button";
import { Cell } from "../../cell";
import { played } from "../../field";
import { Knob } from "../../knob";
import { MenuList } from "../../menu";
import { Select } from "../../select";
import { Toggle } from "../../toggle";
import { Tooltip } from "../../tooltip";
import { DeviceCard } from "../device-card";

const CELL = theme.size.cell;
/** The card's own two controls take three cells, so a plugin's name fits; a list of steps two. */
const [OWN, LIST] = [3, 2];
/** The longest name a switch of one column holds: a longer one takes two, and one longer still ends in an ellipsis with its whole name in a tip. */
const [SWITCH_LETTERS, WIDE_SWITCH_LETTERS] = [7, 17];
/** The most pins a record holds: as many as lanes can move of one device. */
const MOST = 64;

/** `text` in lines of at most `letters`, broken between words: a text does not wrap by itself. */
const lines = (text: string, letters: number) =>
  text.split(" ").reduce<string[]>((all, word) => {
    const last = all.at(-1);
    if (last !== undefined && last.length + 1 + word.length <= letters) all[all.length - 1] = `${last} ${word}`;
    else all.push(word);
    return all;
  }, []);
/** About the letters of a label a cell's width holds. */
const LETTERS_A_CELL = 10;

type Parameter = { id: number; name: string; min: number; max: number; default: number; steps?: number | null; names: { value: number; name: string }[]; automatable: boolean };
type Pin = { name: string; value: number };

/** The value of step `index` of a parameter of `steps` evenly from its min to its max, and the step of `value`. */
const stepValue = (parameter: Parameter, steps: number, index: number) => parameter.min + ((parameter.max - parameter.min) * index) / Math.max(steps - 1, 1);
const stepOf = (parameter: Parameter, steps: number, value: number) => Math.round(((value - parameter.min) / (parameter.max - parameter.min || 1)) * (steps - 1));

/**
 * A plugin: it draws itself in a window of its own, so the card has no display. In its place the
 * button that opens that window over the list of its parameters, which puts one on the card or
 * takes it off; then a control for each pinned parameter, each in the row with the most room. A
 * plugin that cannot play says why where the display would be.
 */
export const card: Card<"plugin"> = (self, ctx) => {
  const { facts, state } = self;
  const title = facts.title;
  const pins = (state.parameters ?? {}) as Record<string, Pin>;
  const says = (text: string) => (
    <col w={OWN * CELL} h={2 * theme.size.row} px={2} justify="center">
      {lines(text, OWN * LETTERS_A_CELL).map((line) => <text style="label" color="text-secondary">{line}</text>)}
    </col>
  );
  const name = facts.name ?? state.plugin_id;
  if (facts.status === "missing") return <DeviceCard id="card" title={title} display={says("This plugin is not on this computer. Install it, or pick another.")} />;
  // The host can start a stopped plugin again, but a card has no way to ask it yet.
  if (facts.status === "stopped") return <DeviceCard id="card" title={title} display={says(`${name} stopped working. Open the project again to start it.`)} />;
  if (facts.status === "not_loaded") return <DeviceCard id="card" title={title} display={says("This plugin did not load, so there is nothing to open.")} />;

  const parameters = new Map(facts.parameters.map((parameter) => [parameter.id, parameter as Parameter]));
  const pinned = Object.entries(pins).filter(([, pin]) => pin).map(([id]) => Number(id)).sort((a, b) => a - b);
  const laned = (id: number) => ctx.held(`parameters.${id}.value`);
  const full = pinned.length >= MOST;
  const set = (id: number, label: string, value: number) =>
    ctx.update(label, (draft) => {
      const pin = (draft.state as { parameters?: Record<string, Pin> }).parameters?.[id];
      if (pin) pin.value = value;
    });
  /** A parameter comes off the card, or goes on it at its default: the card does not ask the plugin what it plays. */
  const pinOrUnpin = (id: number) =>
    ctx.update(pins[id] ? `Remove ${pins[id]!.name}` : `Add ${parameters.get(id)!.name}`, (draft) => {
      const all = ((draft.state as { parameters?: Record<string, Pin> }).parameters ??= {});
      if (all[id]) delete all[id];
      else all[id] = { name: parameters.get(id)!.name, value: parameters.get(id)!.default };
    });

  const window = facts.window === "none"
    ? <col>{lines("This plugin has no window of its own.", OWN * LETTERS_A_CELL).map((line) => <text style="label" color="text-secondary">{line}</text>)}</col>
    : <Button id="plugin-window" label={`${title} ${facts.window === "open" ? "show window" : "open window"}`} text={facts.window === "open" ? "Show window" : "Open window"} size="sm" onPress={() => ctx.openWindow("")} />;
  const listed = [
    ...facts.parameters.map((parameter) => {
      const has = pinned.includes(parameter.id);
      const disabled = has && laned(parameter.id) ? "Automated. Delete its lane to take it off" : !has && full ? "The card is full" : null;
      return { text: parameter.name, key: String(parameter.id), checked: has, disabled, onPress: () => pinOrUnpin(parameter.id) };
    }),
    ...pinned.filter((id) => !parameters.has(id)).map((id) => ({ text: pins[id]!.name, key: String(id), checked: true, detail: "Not in this plugin", onPress: () => pinOrUnpin(id) })),
  ];
  const list = (
    <row key="plugin-parameters" label={`${title} parameters`} role="button" w={OWN * CELL - 8} h={24} px={2} gap={1} align="center" justify="between" radius="md" fill="fill" hover="fill">
      <text style="label" color="text">Parameters</text>
      <icon name="chevron-down" size={14} color="text-muted" />
      <MenuList id="plugin-parameters" label={`${title} parameters`} items={listed} w={260} search notes={full ? [`The card holds ${MOST} parameters at most. Take one off to add another.`] : []} />
    </row>
  );
  const own = (
    <col w={OWN * CELL} gap={1}>
      <Cell span={OWN}>{window}</Cell>
      {facts.parameters.length > 0 && <Cell span={OWN}>{list}</Cell>}
    </col>
  );

  /** The cell of a pin, and how many columns it takes. */
  const control = (id: number): [Node, number] => {
    const pin = pins[id]!;
    const parameter = parameters.get(id);
    if (!parameter) {
      const missing = (
        <row key={`missing-${id}`} label={`${title} ${pin.name} not found`} h={24} align="center">
          <text style="label" color="warning">Not found</text>
          <Tooltip text={`This plugin has no parameter with the id ${id}, so this moves nothing. Take it off in Parameters.`} />
        </row>
      );
      return [<Cell text={pin.name} value={`id ${id}`}>{missing}</Cell>, 1];
    }
    const label = `${title} ${parameter.name}`;
    const change = `Change ${parameter.name}`;
    const outside = pin.value < parameter.min || pin.value > parameter.max;
    const steps = parameter.steps ?? 0;
    if (steps === 2) {
      const on = stepOf(parameter, 2, pin.value) === 1;
      const span = parameter.name.length > SWITCH_LETTERS ? 2 : 1;
      const toggle = <Toggle id={`pin-${id}`} label={label} text={parameter.name} on={on} w={span * CELL - 8} {...(parameter.name.length > WIDE_SWITCH_LETTERS ? { tip: parameter.name } : {})} onToggle={() => set(id, change, stepValue(parameter, 2, on ? 0 : 1))} />;
      return [<Cell span={span} {...(outside ? { value: "Out of range" } : {})}>{toggle}</Cell>, span];
    }
    if (steps > 2 && parameter.names.length >= steps) {
      const options = parameter.names.map((step, index) => ({ value: index, text: step.name }));
      const select = <Select id={`pin-${id}`} label={label} options={options} value={outside ? -1 : stepOf(parameter, steps, pin.value)} w={LIST * CELL - 8} menuW={160} onChange={(index) => set(id, change, stepValue(parameter, steps, index))} />;
      return [<Cell span={LIST} text={parameter.name} {...(outside ? { value: "Out of range" } : {})}>{select}</Cell>, LIST];
    }
    // The plugin's own words for a value need a call into it, which a card cannot make: the number shows.
    const path = `parameters.${id}.value`;
    const held = laned(id) ? live.field(played(path)) : undefined;
    const knob = steps > 2
      ? <Knob id={`pin-${id}`} label={label} text={parameter.name} value={stepOf(parameter, steps, pin.value)} min={0} max={steps - 1} step={1} initial={stepOf(parameter, steps, parameter.default)} onChange={(index) => set(id, change, stepValue(parameter, steps, index))} />
      : <Knob id={`pin-${id}`} path={path} label={label} text={parameter.name} value={pin.value} min={parameter.min} max={parameter.max} initial={parameter.default} {...(held ? { held } : {})} onChange={(value) => set(id, change, Math.min(Math.max(value, parameter.min), parameter.max))} />;
    return [outside ? <Cell text={parameter.name} value="Out of range"><box opacity={0.4}>{knob}</box></Cell> : knob, 1];
  };
  // In the order of their ids, each in the row with the most room.
  const rows: [Node[], Node[]] = [[], []];
  const used = [0, 0];
  for (const id of pinned) {
    const [node, span] = control(id);
    const row = used[0]! <= used[1]! ? 0 : 1;
    rows[row].push(node);
    used[row] = used[row]! + span;
  }
  const controls = pinned.length > 0 && (
    <col w={Math.max(used[0]!, used[1]!) * CELL} gap={1}>
      <row h={theme.size.row}>{rows[0]}</row>
      <row h={theme.size.row}>{rows[1]}</row>
    </col>
  );
  return <DeviceCard id="card" title={title} cells={controls ? [own, controls] : [own]} />;
};
