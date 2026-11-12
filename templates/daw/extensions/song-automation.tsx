import { Circle, For, Fragment, h, Knob, Label, Line, memo, now, Path, positionOf, Rect, Select, Show, signal, Space, valueAt, type Accessor, type Color, type Drag, type Live, type MovedNumber, type Num, type Press } from "./sdk";
import { bent, type AutomationPoint } from "./song-clips";
import { clamp, COLOR, lanesOf, nameFrom, words, type Child, type Page } from "./song-page";
import { ENVELOPE, envelope, lfo, RATES, SHAPES } from "./lfo";
import { freeName, splitTarget, type Mover } from "./track";

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
export const editPoints = ({ update }: Pick<Page, "update">, label: string, track: string, lane: string, change: (points: AutomationPoint[]) => AutomationPoint[]) =>
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
const rangeOf = ({ memory }: Page, track: string, target: string, points: readonly AutomationPoint[]): MovedNumber =>
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
/**
 * A lane's name, and under it what it plays at the playhead, once its number is known; right-click
 * the name to remove the lane.
 */
export const LaneHead = ({ page, track, lane }: { page: Page; track: string; lane: { lane: string; target: string } }) => {
  const played = now.value(`${track}/${lane.target}`, { played: true });
  const known = () => page.memory.ranges[`${track}/${lane.target}`];
  const mover = () => moverOf(page, track, lane.lane);
  const path = `${track}/${lane.lane}`;
  /** The right-click menu of its name: what moves the number, and removing the lane. */
  const menu = async () => {
    const movers = page.rows().find((each) => each.name === track)?.tool === "track" ? MOVED_BY : MOVED_BY.slice(0, 2);
    const items = [...movers.map(([tool, label]) => ({ label, group: "Moved by", checked: mover() === tool })), { label: "Remove lane", separator: true }];
    const picked = await page.openMenu(items);
    const chosen = picked === null ? undefined : movers[picked];
    if (chosen) moveWith(page, track, lane, chosen[0]);
    else if (picked === items.length - 1) removeLane(page, track, lane.lane);
  };
  return (
    <div style={{ grow: true, minWidth: 0, gap: 2 }}>
      <div key={`lane-name-${track}-${lane.lane}`} tooltip={() => laneName(page, track, lane.target, true)} style={{ fontSize: 12, color: "text-secondary", truncate: true }}
        onPress={({ button }) => button === "right" && menu()}>
        {() => laneName(page, track, lane.target)}
      </div>
      {() => {
        if (mover() === "lfo")
          return (
            <div style={{ direction: "row", gap: 4 }}>
              <Select key={`lfo-shape-${path}`} size="sm" field={`${path}.shape`} options={SHAPES.map((shape) => ({ value: shape, label: words(shape) }))} />
              <Select key={`lfo-rate-${path}`} size="sm" field={`${path}.rate`} options={RATES.map((rate) => ({ value: rate, label: rate }))} />
            </div>
          );
        if (mover() === "envelope")
          return (
            <div style={{ direction: "row", gap: 4 }}>
              {Object.entries(ENVELOPE).map(([field, { label, min, max, default: fallback, unit }]) => (
                <Knob key={`env-${field}-${path}`} field={`${path}.${field}`} label={label} size="sm" bare min={min} max={max} default={fallback ?? min} unit={unit} />
              ))}
            </div>
          );
        const number = known();
        if (!number) return null;
        const { unit, times } = readoutOf(number);
        return (
          <Space key={`lane-value-${track}-${lane.lane}`} width={READOUT} height={16}>
            <Label x={0} y={1} text={played.times(times)} unit={unit} size={12} color="text-muted" />
          </Space>
        );
      }}
    </div>
  );
};

/** What can move the number of a lane, as its menu names it: an envelope on a track only, which has notes. */
const MOVED_BY: readonly (readonly [Mover, string])[] = [["lane", "Points"], ["lfo", "LFO"], ["envelope", "Envelope"]];
/** The first letters of each child name, as a lane child is called after what moves it. */
const PREFIX: Record<Mover, string> = { lane: "lane", lfo: "lfo", envelope: "env" };

/** The tool of the child `lane` of `track`: what moves the number. */
const moverOf = ({ rows }: Page, track: string, lane: string): Mover | undefined => {
  const owner = rows().find((each) => each.name === track);
  const child = owner?.tool === "track" || owner?.tool === "tape" ? owner.children.find((each) => each.name === lane) : undefined;
  return child?.tool === "lane" || child?.tool === "lfo" || child?.tool === "envelope" ? child.tool : undefined;
};

/**
 * Moves the number of `lane` with `tool` instead: points that hold what it plays at the start,
 * or an LFO or an envelope between a quarter and three quarters of the travel of the number.
 */
function moveWith({ update, memory }: Page, track: string, lane: { lane: string; target: string }, tool: Mover) {
  const range = memory.ranges[`${track}/${lane.target}`] ?? { min: 0, max: 1, scale: "linear" as const };
  const scale: "linear" | "log" = range.scale === "log" ? "log" : "linear";
  const [low, high] = [laneValue(range, 0.25), laneValue(range, 0.75)];
  update(tool === "lane" ? "Move with points" : tool === "lfo" ? "Move with an LFO" : "Move with an envelope", ({ children }) => {
    const owner = children.find((child) => child.name === track);
    if (owner?.tool !== "track" && owner?.tool !== "tape") return;
    const at = owner.children.findIndex((child) => child.name === lane.lane);
    const old = owner.children[at];
    if (!old || old.tool === tool || (tool === "envelope" && owner.tool !== "track")) return;
    const held = old.tool === "lane" ? (old.state.points[0]?.value ?? low) : old.tool === "lfo" || old.tool === "envelope" ? old.state.low : low;
    owner.children.splice(at, 1);
    const name = freeName(owner.children.map((child) => child.name), `${PREFIX[tool]}-${nameFrom(lane.target)}`);
    if (tool === "lane") owner.children.push({ name: name as `lane-${string}`, tool: "lane", state: { points: [{ tick: 0, value: held }], scale: range.scale } });
    else if (tool === "lfo") owner.children.push({ name: name as `lfo-${string}`, tool: lfo, state: { low, high, scale } });
    else if (owner.tool === "track") owner.children.push({ name: name as `env-${string}`, tool: envelope, state: { low, high, scale } });
    owner.state.automation = owner.state.automation.map((each) => (each.lane === lane.lane ? { ...each, lane: name } : each));
  });
}
/** The line from `from` to `to`, bent by `curve` as the lane plays it, in short straight pieces. */
function bentLine(from: readonly [Num, Num], to: readonly [Num, Num], curve: number) {
  const along = (a: Num, b: Num, part: number): Num => {
    if (typeof a === "number") return typeof b === "number" ? a + (b - a) * part : b.times(part).plus(a * (1 - part));
    return a.times(1 - part).plus(typeof b === "number" ? b * part : b.times(part));
  };
  const pieces = Array.from({ length: 16 }, (_, index) => (index + 1) / 16);
  return [from, ...pieces.map((part) => [along(from[0], to[0], part), along(from[1], to[1], bent(part, curve))] as const)] as const;
}

/** The room of what a lane plays, in its head. */
const READOUT = 96;

/** How a value of a lane's number reads: in the unit its name ends in, seconds as milliseconds. */
function readoutOf(range: MovedNumber): { unit?: "hz" | "db" | "ms" | "pan" | "percent"; times: number } {
  return range.unit === "seconds" ? { unit: "ms", times: 1000 } : { unit: range.unit, times: 1 };
}

/** The row `row` of a lane: its points, or the band of an LFO or an envelope that moves its number instead. */
export const LaneRow = (props: RowProps) => {
  const moved = memo(() => moverOf(props.page, props.track, props.lane.lane));
  return (
    <Show when={() => moved() === "lfo" || moved() === "envelope"} fallback={() => <PointsRow {...props} />}>
      {() => <BandRow {...props} />}
    </Show>
  );
};
type RowProps = { page: Page; track: string; lane: { lane: string; target: string }; row: Accessor<number>; colour: Accessor<Color>; snap: number };

/**
 * The row of an LFO or an envelope: the band its number moves in, between a line at `low` and
 * one at `high`, each of which a drag moves up and down.
 */
const BandRow = ({ page, track, lane, row, colour }: RowProps) => {
  const { rows, topOf, update } = page;
  const band = () => {
    const owner = rows().find((each) => each.name === track);
    const child = owner?.tool === "track" || owner?.tool === "tape" ? owner.children.find((each) => each.name === lane.lane) : undefined;
    return child?.tool === "lfo" || child?.tool === "envelope" ? child.state : { low: 0, high: 1 };
  };
  const range = () => rangeOf(page, track, lane.target, []);
  const top = () => topOf(row());
  const yOf = (value: number) => top() + 0.1 + (1 - positionOf(range(), value)) * 0.8;
  const edge = (end: "low" | "high") => {
    const id = `band-${end}-${track}-${lane.lane}`;
    const moved = now.drag(id);
    const drag: Drag = {
      id,
      onEnd: ({ dy }) => {
        const value = laneValue(range(), positionOf(range(), band()[end]) - dy / 0.8);
        update(end === "low" ? "Change the low" : "Change the high", ({ children }) => {
          const owner = children.find((child) => child.name === track);
          const child = owner?.tool === "track" || owner?.tool === "tape" ? owner.children.find((each) => each.name === lane.lane) : undefined;
          if (child?.tool === "lfo" || child?.tool === "envelope") child.state[end] = value;
        });
      },
    };
    const y = () => moved.dy.plus(yOf(band()[end])).clamp(top() + 0.1, top() + 0.9);
    return [
      <Line from={() => [-1e6, y()] as const} to={() => [1e6, y()] as const} color={colour} width={1.5} />,
      <Rect key={id} x={-1e6} y={() => y().minus(0.06)} width={2e6} height={0.12} color={COLOR.clear} cursor="ns-resize" drag={drag} />,
    ];
  };
  const [low, high] = [() => yOf(band().low), () => yOf(band().high)];
  return (
    <>
      <Rect x={-1e6} y={top} width={2e6} height={page.timeline.pointY} color="border" />
      <Rect x={-1e6} y={() => Math.min(low(), high())} width={2e6} height={() => Math.abs(low() - high())} color="fill" />
      {edge("low")}
      {edge("high")}
    </>
  );
};

/**
 * The points of a lane: lines between them, and a dot per point, bigger under the pointer, with
 * its value beside it. A press in the lane adds a point, on the snap unless cmd is held, and a
 * drag goes on to move it; a drag of a dot moves it between its neighbours; alt-drag erases the
 * points it covers; the dot in the middle of a line bends it, a double-click straightens it.
 */
const PointsRow = ({ page, track, lane, row, colour, snap }: RowProps) => {
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
      editPoints(page, added() ? "Add a point" : "Move a point", track, lane.lane, (all) => all.map((each, at) => (at === index ? { ...each, tick, value } : each)));
      press(tick);
      choose(tick);
    },
  };
  // The line a handle bends, by the tick of the point it starts at.
  const [bending, bendFrom] = signal<number | null>(null);
  const bendId = `bend-${track}-${lane.lane}`;
  const bend = now.drag(bendId);
  const bendDrag: Drag = {
    id: bendId,
    onEnd: ({ dy }) => {
      const tick = bending();
      bendFrom(null);
      const index = points().findIndex((point) => point.tick === tick);
      const [point, after] = [points()[index], points()[index + 1]];
      if (!point || !after) return;
      // The curve whose middle is where the handle was let go.
      const [from, to] = [positionOf(range(), point.value), positionOf(range(), after.value)];
      const middle = from + (to - from) * bent(0.5, point.curve ?? 0) - dy / 0.8;
      const done = clamp((middle - from) / (to - from), 0.01, 0.99);
      const curve = clamp(Math.log(Math.log(done) / Math.log(0.5)) / Math.log(4), -1, 1);
      editPoints(page, "Bend a line", track, lane.lane, (all) => all.map((each) => (each.tick === point.tick ? { ...each, curve } : each)));
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
      <Rect x={-1e6} y={top} width={2e6} height={timeline.pointY} color="border" />
      <Rect x={moved.startX.min(moved.x)} y={() => top() + 0.04} width={moved.dx.abs()} height={0.92} color="fill" border="text/20" borderWidth={1} radius={2}
        opacity={() => (erasing() ? moved.active : 0)} />
      <For each={points}>
        {(point, index) => {
          const last = () => index() === points().length - 1;
          return [
            // Flat before the first point and after the last.
            <Line from={() => [index() === 0 ? 0 : point.tick, at(index())[1]] as const} to={() => at(index())} color={colour} width={1.5} />,
            () => (!point.curve || last()
              ? <Line from={() => at(index())} to={() => (last() ? ([end(), at(index())[1]] as const) : at(index() + 1))} color={colour} width={1.5} />
              : <Path points={() => bentLine(at(index()), at(index() + 1), point.curve ?? 0)} color={colour} width={1.5} />),
          ];
        }}
      </For>
      <For each={points}>
        {(point, index) => {
          // The handle that bends the line to the next point, at its middle: none on a flat line, which no curve bends.
          const key = () => `${bendId}-${point.tick}`;
          const bends = memo(() => {
            const after = points()[index() + 1];
            return after !== undefined && after.value !== point.value;
          });
          const middle = () => {
            const after = points()[index() + 1] ?? point;
            const [from, to] = [positionOf(range(), point.value), positionOf(range(), after.value)];
            return { x: (point.tick + after.tick) / 2, y: top() + 0.1 + (1 - (from + (to - from) * bent(0.5, point.curve ?? 0))) * 0.8 };
          };
          const shown = () => (erasing() ? 0 : now.hover(key()).max(bend.active.times(bending() === point.tick ? 1 : 0)).max(0.35));
          return (
            <Show when={bends}>
              {() => (
                <Circle key={key} x={() => middle().x} y={() => (bending() === point.tick ? bend.dy.plus(middle().y) : middle().y)} radius={4} color={colour} opacity={shown}
                  cursor="ns-resize" drag={bendDrag}
                  onPress={({ button, clicks }) => {
                    if (button !== "left") return;
                    if (clicks === 2) return editPoints(page, "Straighten a line", track, lane.lane, (all) => all.map((each) => (each.tick === point.tick ? { tick: each.tick, value: each.value } : each)));
                    bendFrom(point.tick);
                  }} />
              )}
            </Show>
          );
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
          const readout = () => readoutOf(range());
          const value = () => {
            const [, live] = at(index());
            const up = typeof live === "number" ? undefined : live.minus(top() + 0.1).over(-0.8).plus(1);
            const dragged = up && valueAt(range(), up);
            const held = dragged ? (range().scale === "fader" ? dragged.max(-100) : dragged) : timeline.pointX.times(0).plus(point.value);
            return held.times(readout().times);
          };
          return [
            <Circle x={x} y={y} radius={6} color="text" opacity={() => (chosen() ? 1 : 0)} />,
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
              radius={6} color="background" border="border-strong" borderWidth={1} opacity={shown} />,
            // Its unit is known once the range of the number is.
            () => (
              <Label x={() => timeline.pointX.times(42).plus(x())} y={() => timeline.pointY.times(-7).plus(y())}
                text={value} unit={readout().unit} size={12} color="text" align="center" opacity={shown} />
            ),
          ];
        }}
      </For>
    </>
  );
};
