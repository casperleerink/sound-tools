import { Circle, For, Fragment, h, Label, Line, now, positionOf, Rect, signal, valueAt, type Accessor, type Color, type Drag, type Live, type MovedNumber, type Press } from "./sdk";
import type { LanePoint } from "./song-clips";
import { clamp, COLOR, lanesOf, nameFrom, words, type Child, type Page } from "./song-page";
import { freeName, splitTarget } from "./track";

// The automation of a track: the lanes under it, each a line of points that moves a number of
// the track's fader or of one of its devices over the song.

/** The value a lane holds `up` its row, 0 to 1: a lane holds no -inf, so a volume's bottom is -100 dB. */
const laneValue = (range: MovedNumber, up: number) => {
  const value = valueAt(range, clamp(up, 0, 1));
  return range.scale === "fader" ? Math.max(value, -100) : value;
};
/** Shows or hides the lanes of `each`, and learns the ranges of what they move. */
export function showLanes(page: Page, each: Child, shown: boolean) {
  const { memory } = page;
  const lanes = new Set(memory.lanes);
  if (shown) lanes.add(each.name);
  else lanes.delete(each.name);
  memory.lanes = lanes;
  if (shown) for (const { target } of lanesOf(each)) learnRange(page, each.name, target);
}
async function learnRange(page: Page, track: string, target: string) {
  const { memory, numbers } = page;
  const [child, number] = splitTarget(target);
  const found = (await numbers(`${track}/${child}`)).find((each) => each.name === number);
  if (found) memory.ranges = { ...memory.ranges, [`${track}/${target}`]: found };
  await learnName(page, track, child);
}
export async function learnName({ memory, offers }: Page, track: string, path: string) {
  if (path === "mix") return;
  const { offers: offered, here } = await offers(path === "synth" ? "notes" : "audio", { at: `${track}/${path}` });
  const name = offered.find((each) => each.key === here)?.name;
  if (name) memory.names = { ...memory.names, [`${track}/${path}`]: name };
}
/** What a lane of a track can move, a group each, as the arrangement offered them: the track's own volume and pan, its instrument, then its chain's effects in order. */
function devicesOf({ memory }: Page, each: Child) {
  const effects = each.tool === "track" || each.tool === "tape" ? each.children.find((child) => child.name === "chain") : undefined;
  const order = effects?.tool === "chain" ? effects.state.order : [];
  const paths = [...(each.tool === "track" ? ["synth"] : []), ...order.map((effect) => `chain/${effect}`)];
  const named = (path: string) => memory.names[`${each.name}/${path}`] ?? words(path.split("/").at(-1) ?? path);
  return [["mix", "Track"], ...paths.map((path) => [path, named(path)])] as [string, string][];
}
export async function addLane(page: Page, each: Child) {
  const { memory, update, numbers, openMenu } = page;
  if (each.tool !== "track" && each.tool !== "tape") return;
  await Promise.all(devicesOf(page, each).map(([path]) => learnName(page, each.name, path)));
  const taken = new Set(each.state.automation.map(({ target }) => target));
  const found = (await Promise.all(devicesOf(page, each).map(async ([path, group]) => (await numbers(`${each.name}/${path}`)).map((number) => ({ path, group, number })))))
    .flat()
    // The mute is the solo's, as the arrangement offered no lane for it.
    .filter(({ path, number }) => !taken.has(`${path}.${number.name}`) && !(path === "mix" && number.name === "mute"));
  const label = ({ path, number }: (typeof found)[number]) => (path === "mix" && number.name === "gain_db" ? "Volume" : number.label);
  const picked = await openMenu(found.map((each) => ({ label: label(each), group: each.group })), { width: 240, maxHeight: 320, search: true });
  const chosen = picked === null ? undefined : found[picked];
  if (!chosen) return;
  const target = `${chosen.path}.${chosen.number.name}`;
  const value = chosen.number.value ?? laneValue(chosen.number, 0.5);
  update("Add a lane", ({ children }) => {
    const owner = children.find((child) => child.name === each.name);
    if (owner?.tool !== "track" && owner?.tool !== "tape") return;
    const lane = freeName(owner.children.map((child) => child.name), `lane-${nameFrom(target)}`) as `lane-${string}`;
    owner.children.push({ name: lane, tool: "lane", state: { points: [{ tick: 0, value }], scale: chosen.number.scale } });
    owner.state.automation = [...owner.state.automation, { lane, target }];
  });
  memory.ranges = { ...memory.ranges, [`${each.name}/${target}`]: chosen.number };
  showLanes(page, each, true);
}
const removeLane = ({ update }: Page, track: string, lane: string) =>
  update("Remove a lane", ({ children }) => {
    const owner = children.find((child) => child.name === track);
    if (owner?.tool !== "track" && owner?.tool !== "tape") return;
    owner.state.automation = owner.state.automation.filter((each) => each.lane !== lane);
    const at = owner.children.findIndex((child) => child.name === lane);
    if (at >= 0) owner.children.splice(at, 1);
  });
/** Changes the points of the lane `lane` of `track`, sorted, one a tick; with none left, the lane goes. */
export const editPoints = ({ update }: Pick<Page, "update">, label: string, track: string, lane: string, change: (points: LanePoint[]) => LanePoint[]) =>
  update(label, ({ children }) => {
    const owner = children.find((child) => child.name === track);
    if (owner?.tool !== "track" && owner?.tool !== "tape") return;
    const child = owner.children.find((each) => each.name === lane);
    if (child?.tool !== "lane") return;
    const points = [...new Map(change(child.state.points.map((point) => ({ ...point }))).map((point) => [point.tick, point])).values()].sort((a, b) => a.tick - b.tick);
    if (points.length > 0) child.state.points = points;
    else {
      owner.children.splice(owner.children.indexOf(child), 1);
      owner.state.automation = owner.state.automation.filter((each) => each.lane !== lane);
    }
  });
const rangeOf = ({ memory }: Page, track: string, target: string, points: readonly LanePoint[]): MovedNumber =>
  memory.ranges[`${track}/${target}`] ?? { name: target, label: words(target), value: null, min: Math.min(0, ...points.map((point) => point.value)), max: Math.max(1, ...points.map((point) => point.value)), scale: "linear" };
/**
 * What a lane moves, as a person reads it: `Volume`, `Pan`, or the device and its number,
 * `Filter · Cutoff`. Unless `whole`, a long device is cut short, so the number shows.
 */
const laneName = ({ memory }: Page, track: string, target: string, whole = false) => {
  const [path, number] = splitTarget(target);
  const named = memory.ranges[`${track}/${target}`]?.label ?? words(number);
  if (path === "mix") return number === "gain_db" ? "Volume" : named;
  const device = memory.names[`${track}/${path}`] ?? words(path.split("/").at(-1) ?? path);
  return `${device.length > 14 && !whole ? `${device.slice(0, 11).trimEnd()}…` : device} · ${named}`;
};
/** A lane's name; right-click it to remove the lane. */
export const LaneHead = ({ page, track, lane }: { page: Page; track: string; lane: { lane: string; target: string } }) => (
  <div key={`lane-name-${track}-${lane.lane}`} tooltip={() => laneName(page, track, lane.target, true)} style={{ grow: true, fontSize: 12, color: "gray-800", truncate: true }}
    onPress={({ button }) => button === "right" && page.openMenu([{ label: "Remove lane" }]).then((picked) => picked === 0 && removeLane(page, track, lane.lane))}>
    {() => laneName(page, track, lane.target)}
  </div>
);

/** How the readout of a point shows its value: by the unit its name ends in, or as a share of 0 to 1. */
function readoutOf(target: string, range: MovedNumber): { unit?: "hz" | "db" | "ms" | "pan" | "percent"; times: number } {
  const [, number] = splitTarget(target);
  if (number.endsWith("_seconds")) return { unit: "ms", times: 1000 };
  const unit = /_(hz|db|ms)$/.exec(number)?.[1] as "hz" | "db" | "ms" | undefined;
  return { unit: unit ?? (number === "pan" ? "pan" : range.min === 0 && range.max === 1 ? "percent" : undefined), times: 1 };
}

/**
 * The points of a lane on row `row`: lines between them, and a dot per point, bigger under the
 * pointer, with its value beside it. A press in the lane adds a point, on the snap unless cmd is
 * held, and a drag goes on to move it; a drag of a dot moves it between its neighbours; alt-drag
 * erases the points it covers.
 */
export const LaneRow = ({ page, track, lane, row, colour, snap }: { page: Page; track: string; lane: { lane: string; target: string }; row: Accessor<number>; colour: Accessor<Color>; snap: number }) => {
  const { memory, rows, topOf, end, timeline } = page;
  const points = () => {
    const owner = rows().find((each) => each.name === track);
    const child = owner?.tool === "track" || owner?.tool === "tape" ? owner.children.find((each) => each.name === lane.lane) : undefined;
    return child?.tool === "lane" ? child.state.points : [];
  };
  const range = () => rangeOf(page, track, lane.target, points());
  const top = () => topOf(row());
  const yOf = (value: number) => top() + 0.1 + (1 - positionOf(range(), value)) * 0.8;
  // The point a drag moves, by its tick, whether a press just added it, and whether the drag erases instead.
  const [pressed, press] = signal<number | null>(null);
  const [added, add] = signal(false);
  const [erasing, erase] = signal(false);
  const choose = (tick: number) => {
    memory.selected = new Set();
    memory.point = { track, lane: lane.lane, tick };
  };
  /** The ticks a point may move to: it does not pass its neighbours. */
  const between = (index: number) => [(points()[index - 1]?.tick ?? -1) + 1, (points()[index + 1]?.tick ?? 1e12) - 1] as const;
  const id = `point-${track}-${lane.lane}`;
  const moved = now.drag(id);
  const drag: Drag = {
    id,
    snap: { x: snap || 1, grid: snap > 0 },
    onEnd: ({ startX, x, dx, dy }) => {
      if (erasing()) {
        const [from, to] = [Math.min(startX, x), Math.max(startX, x)];
        editPoints(page, "Erase points", track, lane.lane, (all) => all.filter((point) => point.tick < from || point.tick > to));
        return;
      }
      const index = points().findIndex((point) => point.tick === pressed());
      const point = points()[index];
      if (!point) return;
      const [earliest, latest] = between(index);
      const tick = clamp(point.tick + Math.round(dx), earliest, latest);
      const value = laneValue(range(), positionOf(range(), point.value) - dy / 0.8);
      // Added and moved in one press, it is one undo step.
      editPoints(page, added() ? "Add a point" : "Move a point", track, lane.lane, (all) => all.map((each, at) => (at === index ? { tick, value } : each)));
      press(tick);
      choose(tick);
    },
  };
  /** Where the point at `index` is drawn, live while it is dragged: inside its lane, between its neighbours. */
  const at = (index: number): readonly [number | Live, number | Live] => {
    // A row of a point that went is read once more before it goes.
    const point = points()[index] ?? { tick: 0, value: 0 };
    if (pressed() !== point.tick || erasing()) return [point.tick, yOf(point.value)];
    const [earliest, latest] = between(index);
    return [moved.dx.plus(point.tick).clamp(earliest, latest), moved.dy.plus(yOf(point.value)).clamp(top() + 0.1, top() + 0.9)];
  };
  /** A press where no dot is: alt starts an erase, else a point there, chosen, which the drag moves on. */
  const pressLane = ({ x, y, cmd, alt, button }: Press) => {
    if (button !== "left") return;
    erase(alt);
    add(!alt);
    if (alt) return;
    const tick = Math.max(cmd || snap <= 0 ? Math.round(x) : Math.round(x / snap) * snap, 0);
    editPoints(page, "Add a point", track, lane.lane, (all) => [...all, { tick, value: laneValue(range(), 1 - (y - top() - 0.1) / 0.8) }]);
    press(tick);
    choose(tick);
  };
  return (
    <>
      <Rect x={-1e6} y={top} width={2e6} height={1} color={COLOR.clear} cursor="crosshair" drag={drag} anchor={{ x: 0 }} onPress={pressLane} />
      <Rect x={-1e6} y={top} width={2e6} height={timeline.pointY} color="alpha/5" />
      <Rect x={moved.startX.min(moved.x)} y={() => top() + 0.04} width={moved.dx.abs()} height={0.92} color="alpha/5" border="alpha/20" borderWidth={1} radius={2}
        opacity={() => (erasing() ? moved.active : 0)} />
      <For each={points}>
        {(point, index) => {
          const last = () => index() === points().length - 1;
          return [
            // Flat before the first point and after the last.
            <Line from={() => [index() === 0 ? 0 : point.tick, at(index())[1]] as const} to={() => at(index())} color={colour} width={1.5} />,
            <Line from={() => at(index())} to={() => (last() ? ([end(), at(index())[1]] as const) : at(index() + 1))} color={colour} width={1.5} />,
          ];
        }}
      </For>
      <For each={points}>
        {(point, index) => {
          const key = () => `${id}-${point.tick}`;
          const hovered = () => now.hover(key());
          const chosen = () => memory.point?.track === track && memory.point.lane === lane.lane && memory.point.tick === point.tick;
          const [x, y] = [() => at(index())[0], () => at(index())[1]];
          // The readout shows under the pointer, and while a drag moves the point, also one a press just added.
          const shown = () => (pressed() === point.tick ? hovered().max(moved.active) : hovered());
          const readout = () => readoutOf(lane.target, range());
          const value = () => {
            const [, live] = at(index());
            const up = typeof live === "number" ? undefined : live.minus(top() + 0.1).over(-0.8).plus(1);
            const dragged = up && valueAt(range(), up);
            const held = dragged ? (range().scale === "fader" ? dragged.max(-100) : dragged) : timeline.pointX.times(0).plus(point.value);
            return held.times(readout().times);
          };
          return [
            <Circle x={x} y={y} radius={6} color="gray-950" opacity={() => (chosen() ? 1 : 0)} />,
            <Circle x={x} y={y} radius={() => (chosen() ? 4.5 : hovered().times(1.5).plus(3))} color={colour} />,
            <Circle key={key} x={x} y={y} radius={6} color={COLOR.clear} cursor="pointer" drag={drag} anchor={() => ({ x: point.tick })}
              onPress={({ alt, button }) => {
                if (button !== "left") return;
                erase(false);
                add(false);
                if (alt) return editPoints(page, "Remove a point", track, lane.lane, (all) => all.filter((each) => each.tick !== point.tick));
                press(point.tick);
                choose(point.tick);
              }} />,
            <Rect x={() => timeline.pointX.times(10).plus(x())} y={() => timeline.pointY.times(-11).plus(y())} width={timeline.pointX.times(64)} height={timeline.pointY.times(22)}
              radius={6} color="gray-100" border="alpha/10" borderWidth={1} opacity={shown} />,
            // Its unit is known once the range of the number is.
            () => (
              <Label x={() => timeline.pointX.times(42).plus(x())} y={() => timeline.pointY.times(-7).plus(y())}
                text={value} unit={readout().unit} size={12} color="gray-950" align="center" opacity={shown} />
            ),
          ];
        }}
      </For>
    </>
  );
};
