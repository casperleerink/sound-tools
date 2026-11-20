import { h, live, memo } from "../../../sdk";
import { ENVELOPE, RATES, SHAPES } from "../../../lfo";
import { laneName, MOVED_BY, movablesOf, travelOf, unitOf } from "../../../song/lanes";
import { colourOf, titleOf, words } from "../../../song/model";
import { HEAD, type Page, type Row } from "../../../song/view";
import { Button } from "../../button";
import { control } from "../../field";
import { Knob } from "../../knob";
import { MenuList, type Item } from "../../menu";
import { Popover } from "../../popover";
import { Select } from "../../select";
import { Tooltip } from "../../tooltip";
import { LaneTime } from "./lane-time";

/** The rows of one track's automation: a row for each lane, then the one that adds a lane. */
export type LanesProps = { page: Page; rows: readonly Extract<Row, { kind: "lane" | "add-lane" }>[] };
export type LaneRow = Extract<Row, { kind: "lane" }>;

/** What a lane row draws by: the child that moves its number, how the number travels, and what it is called. */
export function laneOf({ track, lane, target }: LaneRow) {
  const child = track.children.find((each) => each.name === lane);
  const travel = travelOf(track, target);
  return {
    mover: child?.tool === "lane" || child?.tool === "lfo" || child?.tool === "envelope" ? child : undefined,
    travel,
    unit: unitOf(target, travel),
    name: laneName(target),
    title: titleOf(track),
    colour: colourOf(track),
    /** The number it moves, as `live.field` reads what plays. */
    path: `${track.name}/${target}`,
    /** What the keys of its parts start with. */
    id: `${track.name}-${lane}`,
  };
}

/** The heads of a track's lanes, placed in the heads' plane. */
export const LaneHeads = ({ page, rows }: LanesProps) => (
  <box>{rows.map((row) => (row.kind === "lane" ? <LaneHead page={page} row={row} /> : <AddLane page={page} row={row} />))}</box>
);

/** The lanes of a track in time, placed in the timeline's plane. */
export const LaneTimes = ({ page, rows }: LanesProps) => (
  <box>{rows.map((row) => (row.kind === "lane" ? <LaneTime page={page} row={row} /> : <box place={[0, row.top, page.end, row.height]} border="border" borderSides={[0, 0, 1, 0]} />))}</box>
);

/** How far a lane's head is in, so its words start where its track's name does. */
const INDENT = 16;

/** A lane's name as its head shows it: a long device cut short, so the number shows. */
const short = (name: string) => {
  const [device = "", number] = name.split(" · ");
  return number !== undefined && device.length > 14 ? `${device.slice(0, 11).trimEnd()}… · ${number}` : name;
};

/** What a lane's menu does: what moves its number, and taking it away. An envelope only on a track, which has notes. */
export function laneItems(page: Page, row: LaneRow): Item[] {
  const { mover } = laneOf(row);
  const movers = row.track.tool === "track" ? MOVED_BY : MOVED_BY.slice(0, 2);
  const said = { lane: "points", lfo: "an LFO", envelope: "an envelope" } as const;
  return [
    ...movers.map(([tool]) => ({ text: `Moved by ${said[tool]}`, checked: mover?.tool === tool, onPress: () => page.edits.lanes.moveWith(row.track.name, row.lane, tool) })),
    { text: "Remove lane", icon: "trash-2" as const, danger: true, onPress: () => page.edits.lanes.remove(row.track.name, row.lane) },
  ];
}

/**
 * The head of a lane: what it moves, a press or a right press opening its menu; under it what
 * plays now, or the controls of the LFO or the envelope that moves it.
 */
const LaneHead = ({ page, row }: { page: Page; row: LaneRow }) => {
  const lane = laneOf(row);
  const { mover } = lane;
  return memo(`lane-head-${lane.id}`, [row.top, row.height, row.target, row.track.tool, lane.title, mover?.tool, mover?.state], () => {
    const items = laneItems(page, row);
    const cut = short(lane.name);
    return (
      <row key={`lane-head-${lane.id}`} label={`${lane.name} of ${lane.title}`} place={[0, row.top, HEAD - 1, row.height]} px={2} border="border" borderSides={[0, 0, 1, 0]}>
        <box w={INDENT} />
        <col grow={1} gap={1} justify="center" align="start">
          <row key={`lane-name-${lane.id}`} label={`Menu of the ${lane.name} lane of ${lane.title}`} role="button" h={20} px={1} gap={1} align="center" radius="sm" hover="fill">
            <text style="label" color="text-secondary" truncate>{cut}</text>
            <icon name="chevron-down" size={12} color="text-muted" />
            {cut !== lane.name && <Tooltip text={lane.name} />}
            <MenuList id={`lane-menu-${lane.id}`} label={`Menu of the ${lane.name} lane of ${lane.title}`} items={items} />
          </row>
          <MoverControls page={page} row={row} />
        </col>
        <MenuList open="menu" id={`lane-head-menu-${lane.id}`} label={`Menu of the ${lane.name} lane of ${lane.title}`} items={items} />
      </row>
    );
  });
};

/** Under a lane's name: what plays now, the shape and rate of its LFO, or the times of its envelope. */
const MoverControls = ({ page, row }: { page: Page; row: LaneRow }) => {
  const { mover, unit, path, name, id } = laneOf(row);
  const tune = (field: Parameters<typeof page.edits.lanes.tune>[2]) => (value: string | number) => page.edits.lanes.tune(row.track.name, row.lane, field, value);
  if (mover?.tool === "lfo") {
    const { shape, rate } = mover.state;
    return (
      <row gap={1} px={1}>
        <Select id={`lfo-shape-${id}`} label={`Shape of the LFO of ${name}`} w={76} value={shape} options={SHAPES.map((each) => ({ value: each, text: words(each) }))} onChange={tune("shape")} />
        <Select id={`lfo-rate-${id}`} label={`Rate of the LFO of ${name}`} w={56} value={rate} options={RATES.map((each) => ({ value: each, text: each }))} onChange={tune("rate")} />
      </row>
    );
  }
  if (mover?.tool === "envelope") {
    const knobs = Object.entries(ENVELOPE).map(([field, spec]) => {
      const bound = control(page.song.held, `${row.track.name}/${row.lane}.${field}`, spec, mover.state[field as keyof typeof ENVELOPE], tune(field as keyof typeof ENVELOPE));
      return <Knob {...bound} id={`env-${field}-${id}`} label={`${spec.label} of the envelope of ${name}`} text={spec.label} />;
    });
    return (
      <box px={1}>
        <Popover id={`env-${id}`} label={`Shape of the envelope of ${name}`} text="Attack, decay…" size="sm" title="Envelope">
          <row gap={1}>{knobs}</row>
        </Popover>
      </box>
    );
  }
  const playing = live.field(path);
  return <row px={1} h={20} align="center">{unit === undefined ? <text style="number" color="text-muted" w={120} live={playing} digits={2} /> : <text style="number" color="text-muted" w={120} live={playing} unit={unit} />}</row>;
};

/** The row under a track's lanes: a menu of every number a new lane of it can move, by device, with a search line. */
const AddLane = ({ page, row }: { page: Page; row: Extract<Row, { kind: "add-lane" }> }) => {
  const offered = movablesOf(row.track);
  const title = titleOf(row.track);
  return memo(`add-lane-${row.track.name}`, [row.top, row.height, title, offered], () => {
    const groups = [...new Set(offered.map((each) => each.group))];
    return (
      <row place={[0, row.top, HEAD - 1, row.height]} px={2} align="center" border="border" borderSides={[0, 0, 1, 0]}>
        <box w={INDENT - 4} />
        <Button id={`add-lane-${row.track.name}`} label={`Add a lane to ${title}`} text="Add lane" icon="plus" variant="ghost" size="sm">
          <MenuList id={`add-lane-${row.track.name}`} label={`What a new lane of ${title} moves`} w={260} search notes={offered.length === 0 ? ["A lane moves every number already"] : []}
            items={groups.map((group) => ({
              heading: group,
              items: offered.filter((each) => each.group === group).map((each) => ({ key: each.target, label: laneName(each.target, group, each.text), text: each.text, onPress: () => page.edits.lanes.add(row.track.name, each.target, each.value) })),
            }))} />
        </Button>
      </row>
    );
  });
};
