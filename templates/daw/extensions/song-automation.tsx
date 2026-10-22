import { For, Fragment, h, Label, Line, now, positionOf, Rect, Show, signal, valueAt, type Accessor, type Color, type Drag, type Live, type MovedNumber } from "./sdk";
import type { LanePoint } from "./song-clips";
import { BEAT, clamp, lanesOf, nameFrom, type Child, type Page } from "./song-page";
import { freeName, splitTarget } from "./track";

// The automation of a track: the lanes under it, each a line of points that moves a number of
// the track's fader or of one of its devices over the song.

/** A name or a field in plain words, its unit left out: `osc_1.position` is `Osc 1 position`, `cutoff_hz` is `Cutoff`. */
const words = (text: string) =>
  text.replace(/_(hz|db|ms|seconds|cents|octaves|semitones)$/, "").replace(/[._-]+/g, " ").trim().replace(/^./, (first) => first.toUpperCase());

/** The value a lane holds `up` its row, 0 to 1: a lane holds no -inf, so a volume's bottom is -100 dB. */
function laneValue(range: MovedNumber, up: number): number {
  const value = valueAt(range, clamp(up, 0, 1));
  return value === -Infinity ? -100 : value;
}

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
async function learnName({ memory, offers }: Page, track: string, path: string) {
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
/** Changes the points of the lane `lane` of `track`, sorted, one a tick, at least one. */
const editPoints = ({ update }: Page, label: string, track: string, lane: string, change: (points: LanePoint[]) => LanePoint[]) =>
  update(label, ({ children }) => {
    const owner = children.find((child) => child.name === track);
    const child = owner?.tool === "track" || owner?.tool === "tape" ? owner.children.find((each) => each.name === lane) : undefined;
    if (child?.tool !== "lane") return;
    const points = [...new Map(change(child.state.points.map((point) => ({ ...point }))).map((point) => [point.tick, point])).values()].sort((a, b) => a.tick - b.tick);
    if (points.length > 0) child.state.points = points;
  });
const rangeOf = ({ memory }: Page, track: string, target: string, points: readonly LanePoint[]): MovedNumber =>
  memory.ranges[`${track}/${target}`] ?? { name: target, label: words(target), value: null, min: Math.min(0, ...points.map((point) => point.value)), max: Math.max(1, ...points.map((point) => point.value)), scale: "linear" };
/** A point where a lane was pressed, on the grid of the snap unless cmd is held, chosen. */
export function addPoint(page: Page, track: string, { lane, target }: { lane: string; target: string }, x: number, up: number, free: boolean) {
  const { memory, rows } = page;
  const owner = rows().find((each) => each.name === track);
  const child = owner?.tool === "track" || owner?.tool === "tape" ? owner.children.find((each) => each.name === lane) : undefined;
  const points = child?.tool === "lane" ? child.state.points : [];
  const grid = free || memory.snap <= 0 ? 1 : memory.snap;
  const tick = Math.max(Math.round(x / grid) * grid, 0);
  editPoints(page, "Add a point", track, lane, (all) => [...all, { tick, value: laneValue(rangeOf(page, track, target, points), up) }]);
  memory.selected = new Set();
  memory.point = { track, lane, tick };
}

/** What a lane moves, as a person reads it: `Volume`, `Pan`, or the device and its number, `Filter · Cutoff`. */
const laneName = ({ memory }: Page, track: string, target: string) => {
  const [path, number] = splitTarget(target);
  const named = memory.ranges[`${track}/${target}`]?.label ?? words(number);
  if (path === "mix") return number === "gain_db" ? "Volume" : named;
  // The device cut short when it is long, so the number shows.
  const device = memory.names[`${track}/${path}`] ?? words(path.split("/").at(-1) ?? path);
  return `${device.length > 14 ? `${device.slice(0, 11).trimEnd()}...` : device} · ${named}`;
};
/** A lane's name; right-click it to remove the lane. */
export const LaneHead = ({ page, track, lane }: { page: Page; track: string; lane: { lane: string; target: string } }) => (
  <div key={`lane-name-${track}-${lane.lane}`} style={{ grow: true, fontSize: 12, color: "gray-800", truncate: true }}
    onPress={({ button }) => button === "right" && page.openMenu([{ label: "Remove lane" }]).then((picked) => picked === 0 && removeLane(page, track, lane.lane))}>
    {() => laneName(page, track, lane.target)}
  </div>
);

/** The points of a lane on row `row`: lines between them, and a handle per point. */
export const LaneRow = ({ page, track, lane, row, colour }: { page: Page; track: string; lane: { lane: string; target: string }; row: Accessor<number>; colour: Accessor<Color> }) => {
  const { memory, rows, topOf, end, timeline } = page;
  const points = () => {
    const owner = rows().find((each) => each.name === track);
    const child = owner?.tool === "track" || owner?.tool === "tape" ? owner.children.find((each) => each.name === lane.lane) : undefined;
    return child?.tool === "lane" ? child.state.points : [];
  };
  const range = () => rangeOf(page, track, lane.target, points());
  const yOf = (value: number) => topOf(row()) + 0.1 + (1 - positionOf(range(), value)) * 0.8;
  const [pressed, press] = signal<number | null>(null);
  const pointDrag: Drag = {
    id: "point",
    snap: { x: BEAT / 4, grid: true },
    onEnd: ({ dx, dy }) => {
      const moved = points()[pressed() ?? -1];
      if (!moved) return;
      const tick = Math.max(moved.tick + Math.round(dx), 0);
      editPoints(page, "Move a point", track, lane.lane, (all) =>
        all.map((point, index) => (index === pressed() ? { tick, value: laneValue(range(), positionOf(range(), point.value) - dy / 0.8) } : point)),
      );
      memory.point = { track, lane: lane.lane, tick };
      press(null);
    },
  };
  // While a point moves, the lines to it go along.
  const dragged = now.drag("point");
  const at = (index: number): readonly [number | Live, number | Live] => {
    // A row of a point that went is read once more before it goes.
    const point = points()[index] ?? { tick: 0, value: 0 };
    return pressed() === index ? [dragged.dx.plus(point.tick), dragged.dy.plus(yOf(point.value))] : [point.tick, yOf(point.value)];
  };
  const chosen = () => {
    const point = memory.point;
    return point?.track === track && point.lane === lane.lane ? points().findIndex((each) => each.tick === point.tick) : -1;
  };
  /** How the readout of a point shows its value: by the unit its name ends in, or as a share of 0 to 1. */
  const unitOf = () => {
    const [, number] = splitTarget(lane.target);
    const found = (/_(hz|db|ms)$/.exec(number)?.[1] ?? (number === "pan" ? "pan" : undefined)) as "hz" | "db" | "ms" | "pan" | undefined;
    return found ?? (range().min === 0 && range().max === 1 ? "percent" : undefined);
  };
  return (
    <>
      <Rect x={-1e6} y={() => topOf(row())} width={2e6} height={timeline.pointY} color="alpha/5" />
      <For each={points}>
        {(point, index) => {
          const last = () => index() === points().length - 1;
          return [
            // Flat before the first point and after the last.
            <Line from={() => [index() === 0 ? 0 : point.tick, at(index())[1]] as const} to={() => at(index())} color={colour} width={1.5} />,
            <Line from={() => at(index())} to={() => (last() ? ([end(), at(index())[1]] as const) : at(index() + 1))} color={colour} width={1.5} />,
            <Rect x={() => timeline.pointX.times(-3.5).plus(point.tick)} y={() => timeline.pointY.times(-3.5).plus(yOf(point.value))}
              width={timeline.pointX.times(7)} height={timeline.pointY.times(7)} radius={3.5} color={colour} cursor="grab"
              drag={pointDrag} anchor={() => ({ x: point.tick })} moves={() => (pressed() === index() ? ["point"] : undefined)}
              onPress={({ alt }) => {
                if (alt) return points().length > 1 && editPoints(page, "Remove a point", track, lane.lane, (all) => all.filter((_, each) => each !== index()));
                press(index());
                memory.selected = new Set();
                memory.point = { track, lane: lane.lane, tick: point.tick };
              }} />,
          ];
        }}
      </For>
      <Show when={() => chosen() >= 0}>
        {() => {
          // By the point as it moves; its readout says what it holds.
          const [x, y] = [() => timeline.pointX.times(0).plus(at(chosen())[0]), () => timeline.pointY.times(0).plus(at(chosen())[1])];
          const value = () => points()[chosen()]?.value ?? 0;
          return [
            <Rect x={() => x().plus(timeline.pointX.times(-5))} y={() => y().plus(timeline.pointY.times(-5))}
              width={timeline.pointX.times(10)} height={timeline.pointY.times(10)} radius={5} color="gray-950" border={colour} borderWidth={1.5} />,
            <Rect x={() => x().plus(timeline.pointX.times(12))} y={() => y().plus(timeline.pointY.times(-11))} width={timeline.pointX.times(64)} height={timeline.pointY.times(22)}
              radius={6} color="gray-100" border="alpha/10" borderWidth={1} />,
            <Label x={() => x().plus(timeline.pointX.times(44))} y={() => y().plus(timeline.pointY.times(-7))}
              text={() => timeline.pointX.times(0).plus(value())} unit={unitOf()} size={12} color="gray-950" align="center" />,
          ];
        }}
      </Show>
    </>
  );
};
