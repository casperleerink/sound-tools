import { h, live, Live, memo, type Color, type Live as LiveValue, type Num, type Pointer } from "../../../sdk";
import { laneValue, roomOf, type AutomationPoint, type Travel } from "../../../song/lanes";
import { placeOf } from "../../../song/time";
import { PLANE, ROW, type Page } from "../../../song/view";
import { MenuList } from "../../menu";
import { Tooltip } from "../../tooltip";
import { liveTravelled, liveValueAt, shown } from "../../value";
import { dotsOf, downOf, fine, lineOf, MARGIN, middleOf, PIECE, piecesOf, waveOf } from "./lane-shapes";
import { laneItems, laneOf, type LaneRow } from "./lanes";

/** Room over the top of a lane's travel and under its bottom, a share of its row, so a point there shows whole. */
const PAD = 0.12;
/** How tall the line of a band's edge is to grab, in rows. */
const EDGE = 10 / ROW;
/** A point's mark, with a ring of the background round its dot, and a bend's handle, in points. */
const [MARK, DOT] = [11, 8];

/**
 * Where a lane's row is in the song's plane: `y` of a place `down` its travel (0 its top, 1 its
 * bottom), as rows of the plane; `down` of a pointer, and of a drag's `dy`.
 */
function placesOf(row: LaneRow) {
  const tall = row.height * (1 - 2 * PAD);
  const top = row.top + row.height * PAD;
  return {
    tall,
    y: (down: number) => fine(top + down * tall),
    liveY: (down: Live) => down.times(tall).plus(top),
    down: (y: number) => (y - top) / tall,
    /** A pointer in the lane's units, as the edits take it. */
    inLane: (p: Pointer): Pointer => ({ ...p, y: (p.y - top) / tall, dy: p.dy / tall }),
  };
}

/**
 * A lane in time, on the song's plane: its points and the line through them, or the band an LFO
 * or an envelope moves the number in; while the song plays, a dot where the number is.
 */
export const LaneTime = ({ page, row }: { page: Page; row: LaneRow }) => {
  const lane = laneOf(row);
  const { song, end } = page;
  // Only the open lane's points take the pointer: a song's every point as an element is more than the window takes at once.
  const open = song.memory.point?.track === row.track.name && song.memory.point.lane === row.lane;
  const chosen = open ? song.memory.point!.ticks : [];
  const runs = song.clock.tempo_map.time_signatures;
  return memo(`lane-time-${lane.id}`, [row.top, row.height, end, lane.mover?.tool, lane.mover?.state, song.memory.snap, open, chosen.join(), lane.colour, row.target, runs], () => {
    const key = `lane-${lane.id}`;
    const at = placesOf(row);
    const marquee = live.drag(key);
    const from = marquee.x.min(marquee.x.plus(marquee.dx));
    const say = (tick: number) => {
      const [bar, beat, sixteenth] = placeOf(runs, tick);
      return `${bar}.${beat}${sixteenth > 1 ? `.${sixteenth}` : ""}`;
    };
    const points = lane.mover?.tool === "lane" ? lane.mover.state.points : [];
    const band = lane.mover?.tool === "lfo" || lane.mover?.tool === "envelope";
    const lanes = page.edits.lanes;
    const [track, name] = [row.track.name, row.lane];
    return (
      <box key={key} label={`${lane.name} lane of ${lane.title}`} place={[0, row.top, end, row.height]} fill={open ? "fill/40" : undefined} border="border" borderSides={[0, 0, 1, 0]}
        drag={band ? undefined : { x: true, y: true }}
        onPress={(p) => {
          if (band || p.right) return;
          if (p.clicks === 2) lanes.addPoint(track, name, at.inLane(p));
          else lanes.choosePoints(track, name, { x: p.x, dx: 0, shift: p.shift });
        }}
        onDragEnd={(p) => (p.alt ? lanes.erase(track, name, p) : lanes.choosePoints(track, name, p))}>
        {points.length === 1 && (
          <box place={[live.plane(PLANE).left.max(0), row.top, 1, row.height]} inert>
            <text at={[8, 6]} style="label" color="text-muted" w={420}>Double-click to add a point, drag over points to choose them</text>
          </box>
        )}
        {band ? <Band page={page} row={row} /> : <Points page={page} row={row} points={points} open={open} chosen={chosen} say={say} />}
        {!band && <box place={[from, row.top, marquee.dx.abs(), row.height]} fill={`${lane.colour}/15` as Color} border={`${lane.colour}/60` as Color} opacity={marquee.active.times(Live.of(1).minus(marquee.alt))} />}
        {!band && <box place={[from, row.top, marquee.dx.abs(), row.height]} fill="danger/15" border="danger/60" opacity={marquee.active.times(marquee.alt)} />}
        <box place={[live.playhead, at.liveY(liveTravelled(live.field(lane.path), lane.travel).negate().plus(1))]} w={DOT} h={DOT} radius="full" fill="text" opacity={live.playing} />
        <MenuList open="menu" id={`lane-time-menu-${lane.id}`} label={`Menu of the ${lane.name} lane of ${lane.title}`} items={laneItems(page, row)} />
      </box>
    );
  });
};

/**
 * The points of a lane and the line through them. In the open lane a point drags in time on the
 * snap and in value (finer with Shift), with the others chosen with it. A sloped line beside a chosen point has a
 * handle in its middle that bends it, and a double click on it straightens it.
 */
const Points = ({ page, row, points, open, chosen, say }: { page: Page; row: LaneRow; points: readonly AutomationPoint[]; open: boolean; chosen: readonly number[]; say: (tick: number) => string }) => {
  const { travel, colour, unit, id } = laneOf(row);
  const at = placesOf(row);
  const snap = page.song.memory.snap;
  const lanes = page.edits.lanes;
  const [track, name] = [row.track.name, row.lane];
  const downs = points.filter((point) => chosen.includes(point.tick)).map((point) => downOf(point.value, travel));
  const together = chosen.length > 1;
  // The drag a point moves by: the chosen move as one.
  const dragOf = (tick: number) => live.drag(together && chosen.includes(tick) ? `points-${id}` : `point-${id}-${tick}`);
  return (
    <box>
      {piecesOf(lineOf(points, travel, page.end)).map((piece) => (
        <shapes place={[piece.start - MARGIN, at.y(0), PIECE + 2 * MARGIN, at.tall]} units={[PIECE + 2 * MARGIN, 1]}
          lines={[{ color: colour, width: 1.5, points: open ? following(piece, points, travel, page.end, dragOf, at.tall) : piece.points, fill: `${colour}/10` as Color }]} />
      ))}
      {!open && piecesOf(dotsOf(points, travel)).map((piece) => (
        <shapes place={[piece.start - MARGIN, at.y(0), PIECE + 2 * MARGIN, at.tall]} units={[PIECE + 2 * MARGIN, 1]} lines={[{ color: colour, width: DOT - 1, points: piece.points }]} />
      ))}
      {points.slice(0, -1).map((point, index) => {
        const next = points[index + 1]!;
        // A line bends from beside a chosen point; a flat one has nothing to bend.
        if (next.value === point.value || !(chosen.includes(point.tick) || chosen.includes(next.tick))) return null;
        const key = `bend-${id}-${point.tick}`;
        const down = middleOf(point, next, travel);
        // Faint at rest, whole under the pointer. A drag stays in its bounds, so its place needs no clamp.
        return (
          <box key={key} label={`Bend the line after the point at ${say(point.tick)}`} role="slider" value={point.curve ?? 0} place={[(point.tick + next.tick) / 2, live.drag(key).dy.plus(at.y(down))]} w={DOT} h={DOT} radius="full" fill={`${colour}/35` as Color} hover={colour}
            drag={{ y: [-down * at.tall, (1 - down) * at.tall] }} onDragEnd={(p) => lanes.bend(track, name, point.tick, at.inLane(p))} onPress={(p) => p.clicks === 2 && lanes.straighten(track, name, point.tick)} />
        );
      })}
      {open && points.map((point) => {
        const key = `point-${id}-${point.tick}`;
        const isChosen = chosen.includes(point.tick);
        const down = downOf(point.value, travel);
        // The chosen points move as one, by one drag.
        const moved = dragOf(point.tick);
        const [earliest, latest] = roomOf(points, together && isChosen ? chosen : [point.tick]);
        const [top, bottom] = together && isChosen ? [Math.min(...downs), Math.max(...downs)] : [down, down];
        // Each point is a few hundred bytes of the tree: only a chosen one says what it holds as it drags; the others in a tip.
        const value = shown(laneValue(travel, 1 - down), unit);
        return (
          <box key={key} label={`Point at ${say(point.tick)}, ${value}`} role="button" checked={isChosen}
            place={[moved.dx.plus(point.tick), moved.dy.plus(at.y(down))]} w={MARK} h={MARK} radius="full" fill={isChosen ? "text" : colour} border="background" hover="text/40"
            drag={{ x: [earliest, latest], y: [fine(-top * at.tall), fine((1 - bottom) * at.tall)], ...(snap > 0 ? { snap: [snap, 0] as [number, number] } : {}), ...(together && isChosen ? { id: `points-${id}` } : {}) }}
            onPress={(p) => lanes.pressPoint(track, name, point.tick, p)} onDragEnd={(p) => lanes.movePoint(track, name, point.tick, at.inLane(p))}>
            {isChosen ? (
              <row at={[MARK, -24]} h={22} px={2} align="center" radius="md" fill="raised" border="border-strong" inert>
                <Readout lane={laneOf(row)} down={moved.dy.over(at.tall).plus(down)} />
              </row>
            ) : <Tooltip text={value} />}
          </box>
        );
      })}
    </box>
  );
};

/**
 * The numbers of a piece of the line with its ends at a point where that point's drag has it,
 * and the flat ends of the line at the height of the first and the last: the line follows a
 * point as it moves.
 */
function following(piece: { start: number; points: readonly number[] }, points: readonly AutomationPoint[], travel: Travel, end: number, dragOf: (tick: number) => ReturnType<typeof live.drag>, tall: number): Num[] {
  const [first, last] = [points[0], points.at(-1)];
  const out: Num[] = [];
  for (let at = 0; at + 1 < piece.points.length; at += 2) {
    const [x, y] = [piece.points[at]!, piece.points[at + 1]!];
    const tick = x + piece.start - MARGIN;
    const point = points.find((each) => each.tick === tick && downOf(each.value, travel) === y);
    // The flat ends before the first point and after the last follow their height only.
    const flat = tick === 0 ? first : tick === Math.max(end, last?.tick ?? 0) ? last : undefined;
    const by = point ?? flat ? dragOf((point ?? flat)!.tick) : undefined;
    out.push(point && by ? by.dx.plus(x) : x, by ? by.dy.over(tall).plus(y) : y);
  }
  return out;
}

/** What a chosen point holds `down` its lane as it drags, in its unit: what a release there sets. */
const Readout = ({ lane: { travel, unit }, down }: { lane: ReturnType<typeof laneOf>; down: LiveValue }) => {
  // To a hundredth: a live value's numbers are 32-bit, and 0 dB would read -0.0000005 dB.
  const value = liveValueAt(down.clamp(0, 1).negate().plus(1), travel).times(100).round().over(100);
  return unit === undefined ? <text style="number" color="text" w={72} live={value} digits={2} /> : <text style="number" color="text" w={72} live={value} unit={unit} />;
};

/**
 * The band an LFO or an envelope moves the number in: a drag moves its low or its high edge, and
 * the arrows step one. An LFO draws the wave it plays in it, and hides it while an edge moves.
 */
const Band = ({ page, row }: { page: Page; row: LaneRow }) => {
  const { mover, travel, colour, unit, name, id } = laneOf(row);
  if (mover?.tool !== "lfo" && mover?.tool !== "envelope") return null;
  const at = placesOf(row);
  const keys = { low: `band-low-${id}`, high: `band-high-${id}` };
  const downAt = (end: "low" | "high") => live.drag(keys[end]).dy.over(at.tall).plus(downOf(mover.state[end], travel)).clamp(0, 1);
  const [low, high] = [at.liveY(downAt("low")), at.liveY(downAt("high"))];
  const still = Live.of(1).minus(live.drag(keys.low).active.max(live.drag(keys.high).active));
  const wave = mover.tool === "lfo" ? waveOf(mover.state, travel, page.end) : [];
  const edge = (end: "low" | "high") => {
    const down = downOf(mover.state[end], travel);
    const step = (by: number) => page.edits.lanes.band(row.track.name, row.lane, end, { dy: -by * 0.01 });
    return (
      <box key={keys[end]} label={`${end === "low" ? "Low" : "High"} of the ${mover.tool === "lfo" ? "LFO" : "envelope"} of ${name}`} role="slider" value={mover.state[end]}
        place={[0, (end === "low" ? low : high).minus(EDGE / 2), page.end, EDGE]} drag={{ y: [-down * at.tall, (1 - down) * at.tall] }} hover={`${colour}/15` as Color}
        onDragEnd={(p) => page.edits.lanes.band(row.track.name, row.lane, end, at.inLane(p))} actions={{ increment: step, decrement: (by) => step(-by) }}>
        <box at={[0, (EDGE * ROW) / 2 - 0.75]} w="full" h={1.5} fill={colour} opacity={wave.length > 0 ? 0.5 : 1} />
      </box>
    );
  };
  // What each edge holds, at the left of the window.
  const said = (end: "low" | "high") => (
    <row place={[live.plane(PLANE).left.max(0), (end === "low" ? low : high).minus(end === "high" ? 16 / ROW : 0), 1, 16 / ROW]} px={2} align="center" inert>
      <text style="label" color="text-muted" w={80}>{unit === undefined ? shown(mover.state[end]) : shown(mover.state[end], unit)}</text>
    </row>
  );
  return (
    <box>
      <box place={[0, low.min(high), page.end, low.minus(high).abs()]} fill={`${colour}/10` as Color} />
      {piecesOf(wave).map((piece) => <shapes place={[piece.start - MARGIN, at.y(0), PIECE + 2 * MARGIN, at.tall]} units={[PIECE + 2 * MARGIN, 1]} lines={[{ color: colour, width: 1.5, points: piece.points }]} opacity={still} />)}
      {edge("high")}
      {edge("low")}
      {said("high")}
      {said("low")}
    </box>
  );
};
