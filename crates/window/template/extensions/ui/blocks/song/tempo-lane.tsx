import { h, live, memo, type Color } from "../../../sdk";
import type { Mark } from "../../../song/model";
import { barNear, nearestOf } from "../../../song/time";
import { ACROSS, GUTTER, HEAD, PLANE, TEMPO, type Page } from "../../../song/view";
import { bpmText, pointed, TimeMenu } from "./time-menu";

/** The room above the fastest tempo and under the slowest, and the least tempos the lane's height spans. */
const [EDGE, SPAN] = [6, 24];

/**
 * The tempo lane under the ruler: the tempo as a stepped line, the slowest at its foot, its value
 * at the left edge while it holds. A double press adds a change, a change drags in time and up
 * or down in tempo, Alt-press removes it, and a press chooses it and plays from it.
 */
export const TempoLane = ({ page }: { page: Page }) => {
  const { song, edits, end } = page;
  const { clock, memory } = song;
  const changes = clock.tempo_map.tempo_changes;
  const tall = TEMPO - 1;
  const bpms = changes.map((change) => change.bpm);
  const [low, high] = [Math.min(...bpms), Math.max(...bpms)];
  const yOf = (bpm: number) => (high > low ? EDGE + ((high - bpm) / (high - low)) * (tall - 2 * EDGE) : tall / 2);
  const perPoint = Math.max(high - low, SPAN) / (tall - 2 * EDGE);
  const view = live.plane(PLANE);
  return (
    <row w="full" h={TEMPO} border="border" borderSides={[0, 0, 1, 0]}>
      <row w={HEAD} h="full" px={3} gap={1} align="center" border="border" borderSides={[0, 1, 0, 0]}>
        <text style="label" color="text">Tempo</text>
        <row grow={1} gap={1} justify="end" align="center">
          <text live={live.bpm} digits={0} style="number" color="text-secondary" />
          <text style="label" color="text-muted">bpm</text>
        </row>
      </row>
      <box w={GUTTER} />
      <box key="tempo" label="Tempo lane" grow={1} h="full" plane={{ x: PLANE }}
        onPress={(p) => (p.right ? pointed(page, p.x) : p.clicks === 2 ? edits.marks.addTempo(nearestOf(p.x, memory.snap)) : edits.marks.press({ x: p.x, clicks: 1 }))}>
        {memo("tempo-line", [changes, end, memory.mark], () => (
          <box>
            <shapes place={[0, 0, end, tall]} lines={[{ color: "text-secondary", width: 1.5, points: changes.flatMap((change, index) => {
              const next = changes[index + 1];
              const y = yOf(change.bpm);
              return [change.tick, y, next?.tick ?? end, y, ...(next ? [next.tick, y, next.tick, yOf(next.bpm)] : [])];
            }) }]} />
            {changes.map((change, index) => {
              const [y, to] = [yOf(change.bpm), changes[index + 1]?.tick ?? end];
              // Not under its own pill, which says it while it is chosen.
              if (memory.mark?.kind === "tempo" && memory.mark.tick === change.tick) return null;
              return (
                <box place={[change.tick, y > 16 ? y - 15 : y + 2, to - change.tick, 14]} clip inert>
                  <text at={[view.left.minus(change.tick).max(0).times(view.zoom.times(ACROSS)).plus(12), -1]} style="label" color="text-muted">{bpmText(change.bpm)}</text>
                </box>
              );
            })}
          </box>
        ))}
        {changes.map((change) => {
          const bar = barNear(clock.tempo_map.time_signatures, change.tick).bar;
          const mark: Mark = { kind: "tempo", bar, tick: change.tick };
          const key = `tempo-point-${change.tick}`;
          const moved = live.drag(key);
          const chosen = memory.mark?.kind === "tempo" && memory.mark.tick === change.tick;
          const first = change.tick === 0;
          const ink: Color = chosen ? "text" : "text-secondary";
          const pill = (
            <row at={[12, -1]} h={18} px={1} align="center" radius="sm" fill="background" border="border-strong">
              <text style="label" color="text">{`${bpmText(change.bpm)} bpm`}</text>
            </row>
          );
          const set = (by: number) => edits.marks.moveMark(mark, 0, change.bpm + by);
          return (
            <box key={key} label={first ? "Tempo at the start" : `Tempo at bar ${bar}`} role="slider" value={change.bpm} hoverGroup place={[moved.dx.plus(change.tick), moved.dy.plus(yOf(change.bpm))]} w={15} h={15}
              drag={first ? { y: true } : { x: [1 - change.tick, 1e7], y: true, ...(memory.snap > 0 ? { snap: [memory.snap, 0] as [number, number], from: [change.tick, 0] as [number, number] } : {}) }}
              onPress={(p) => (p.right ? pointed(page, p.x, first ? null : mark) : first ? edits.marks.press({ x: 0, clicks: 1 }) : p.alt ? edits.marks.remove(mark) : edits.marks.pressMark(mark, p))}
              onDragEnd={(p) => edits.marks.moveMark(mark, first ? 0 : p.dx, p.dy === 0 ? undefined : Math.round(change.bpm - p.dy * perPoint))}
              actions={{ increment: set, decrement: (by) => set(-by) }}>
              <box at={[3, 3]} w={9} h={9} radius="full" fill={chosen ? ink : "background"} border={ink} />
              {chosen ? pill : <box reveal>{pill}</box>}
            </box>
          );
        })}
        <box place={[live.playhead, tall / 2]} w={1} h={tall} fill="text" inert />
        <TimeMenu page={page} id="tempo-menu" label="Tempo lane menu" />
      </box>
    </row>
  );
};
