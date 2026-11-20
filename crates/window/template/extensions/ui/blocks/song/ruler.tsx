import { h, live, memo, type Color, type Num } from "../../../sdk";
import { changeBars } from "../../../song/marks";
import type { Mark } from "../../../song/model";
import { barAt, barNear, barStarts, BAR, BEAT, beatRuns, SNAPS, type Runs } from "../../../song/time";
import { ACROSS, GUTTER, HEAD, PLANE, RULER, ZOOM, type Page } from "../../../song/view";
import { Select } from "../../select";
import { Toggle } from "../../toggle";
import { TempoLane } from "./tempo-lane";
import { bpmText, pointed, TimeMenu } from "./time-menu";

export type RulerProps = { page: Page };

/** The line the sections stand on, the top of the bar numbers, and the foot where the bars, the tempo changes and the repeat are. */
const [LINE, NUMBERS, FOOT] = [18, 21, RULER - 8];
/** The least points between two bar numbers, and between a label and the next. */
const [APART, GAP] = [40, 6];

/**
 * The band over the tracks: the snap and the tempo lane's switch in its corner; the bars of the
 * song, its sections on the top line, its time signature and tempo changes, and the repeat at its
 * foot; the tempo lane under it while it shows. It shares the timeline's time.
 */
export const Ruler = ({ page }: RulerProps) => {
  const { memory } = page.song;
  return (
    <col w="full">
      <row w="full" h={RULER} border="border" borderSides={[0, 0, 1, 0]}>
        <row w={HEAD} h="full" px={2} gap={2} align="center" border="border" borderSides={[0, 1, 0, 0]}>
          <Toggle id="tempo-lane" label="Tempo lane" icon="tempo-change" tip="Tempo" on={memory.tempoLane} onToggle={() => (memory.tempoLane = !memory.tempoLane)} />
          <text style="label" color="text-muted">Snap</text>
          <row grow={1} justify="end">
            <Select id="snap" label="Snap" w={72} options={SNAPS.map(({ label, ticks }) => ({ value: ticks, text: label }))} value={memory.snap} onChange={(ticks) => (memory.snap = ticks)} />
          </row>
        </row>
        <box w={GUTTER} />
        <Bars page={page} />
      </row>
      {memory.tempoLane && <TempoLane page={page} />}
    </col>
  );
};

/** The ruler in time: a press plays from there, a double press adds a section, a drag repeats what it went over. */
const Bars = ({ page }: RulerProps) => {
  const { song, edits, end } = page;
  const { clock, memory } = song;
  const runs = clock.tempo_map.time_signatures;
  const loop = live.drag("ruler");
  const [from, to] = [loop.x.snap(BEAT), loop.x.plus(loop.dx).snap(BEAT)];
  return (
    <box key="ruler" label="Ruler" grow={1} h={RULER} plane={{ x: PLANE }} drag={{ x: true, snap: [BEAT, 1], from: [0, 0] }}
      onPress={(p) => (p.right ? pointed(page, p.x) : edits.marks.press(p))} onDragEnd={(p) => edits.marks.loop(p)}>
      {memo("ruler-bars", [runs, end], () => <Grid runs={runs} end={end} />)}
      {memo("ruler-sections", [clock.sections, runs, end, memory.mark, memory.renamingSection], () => <Sections page={page} />)}
      {memo("ruler-marks", [runs, clock.tempo_map.tempo_changes, memory.mark, memory.tempoLane, memory.snap], () => <Changes page={page} />)}
      <Repeat page={page} />
      <box place={[from.min(to), RULER - 6, to.minus(from).abs(), 3]} radius="sm" fill="text" opacity={loop.active} inert />
      <box place={[live.playhead, RULER - 5]} w={7} h={7} radius="full" fill="text" inert />
      <TimeMenu page={page} id="ruler-menu" label="Ruler menu" />
    </box>
  );
};

/** 1 while `points` fit in `ticks` at the zoom of the song's time: what thins labels as it zooms out, with no run of the view. */
function fits(points: number, ticks: number): Num {
  const zoom = points / (Math.max(ticks, 1) * ACROSS);
  return zoom <= ZOOM[0] ? 1 : live.plane(PLANE).zoom.ge(zoom);
}

const both = (a: Num, b: Num): Num => (typeof a === "number" ? (a === 1 ? b : 0) : typeof b === "number" ? (b === 1 ? a : 0) : a.times(b));

/** About how many points a label takes, for the room a time signature's needs. */
const widthOf = (text: string) => text.length * 6.5 + 10;

/** The bars and beats as ticks at the foot, and the numbers of the bars: as the song zooms out, every second, fourth, eighth … shows, the first always. */
const Grid = ({ runs, end }: { runs: Runs; end: number }) => {
  const starts = barStarts(runs, end);
  const changes = runs.length > 1 ? changeBars(runs) : [];
  const beatsOf = beatRuns(runs);
  const [bars, beats]: [number[], number[]] = [[], []];
  starts.forEach((tick, index) => {
    bars.push(tick, 0, tick, 8);
    const beat = beatsOf.findLast((run) => run.from <= tick)?.ticks ?? BEAT;
    for (let at = tick + beat; at < (starts[index + 1] ?? end); at += beat) beats.push(at, 4, at, 8);
  });
  return (
    <box key="ruler-numbers">
      <shapes place={[0, FOOT, end, 8]} lines={[{ color: "border-strong", width: 1, points: bars }]} />
      <shapes place={[0, FOOT, end, 8]} lines={[{ color: "border", width: 1, points: beats }]} opacity={fits(10, BEAT)} />
      {starts.slice(0, -1).map((tick, index) => {
        const bar = index + 1;
        if (changes.includes(bar)) return null;
        const apart = index & -index;
        const next = starts[index + apart] ?? tick + apart * BAR;
        // A time signature's label before it takes its room first.
        const before = changes.findLast((change) => change < bar);
        const crowd = before === undefined ? 1 : fits(widthOf(`${before}  ${runs[changes.indexOf(before)]!.signature}`) + GAP, tick - starts[before - 1]!);
        const shows = index === 0 ? 1 : fits(APART, next - tick);
        return (
          <box place={[tick, NUMBERS, (starts[index + 1] ?? end) - tick, 16]} opacity={both(shows, crowd)} inert>
            <text at={[5, 0]} style="label" color="text-muted">{bar}</text>
          </box>
        );
      })}
    </box>
  );
};

/** Whether `mark` is the chosen one. */
const isChosen = (page: Page, kind: Mark["kind"], tick: number) => page.song.memory.mark?.kind === kind && page.song.memory.mark.tick === tick;

/** A mark's press: a right press opens the menu on it, Alt removes it, else it is chosen and played from. */
const pressOf = (page: Page, mark: Mark) => (p: { right: boolean; alt: boolean; clicks: number; x: number }) => {
  if (p.right) pointed(page, p.x, mark);
  else if (p.alt) page.edits.marks.remove(mark);
  else page.edits.marks.pressMark(mark, p);
};

/** How far a drag of a mark at `tick` steps: a bar for a section or a time signature, the snap for a tempo change. */
const dragOf = (tick: number, step: number, least: number) => ({ x: [least - tick, 1e7] as [number, number], ...(step > 0 ? { snap: [step, 1] as [number, number], from: [tick, 0] as [number, number] } : {}) });

/**
 * The sections on the top line: a tick at its bar, its name at the left edge while the section
 * goes on past it, cut at its end. A press chooses it and plays from it, a double press names it,
 * a drag moves it to another bar.
 */
const Sections = ({ page }: RulerProps) => {
  const { song, end, edits } = page;
  const { clock, memory } = song;
  const runs = clock.tempo_map.time_signatures;
  const view = live.plane(PLANE);
  return (
    <box>
      {clock.sections.map((section, index) => {
        const [from, to] = [barStarts(runs, end)[section.bar - 1] ?? end, barStarts(runs, end)[(clock.sections[index + 1]?.bar ?? Infinity) - 1] ?? end];
        const mark: Mark = { kind: "section", bar: section.bar, tick: from };
        const key = `section-${section.bar}`;
        const moved = live.drag(key).dx.plus(from);
        const chosen = isChosen(page, "section", from);
        const { start, end: next } = barAt(runs, from);
        if (memory.renamingSection?.bar === section.bar) {
          return (
            <box place={[from, 0, 0, 0]}>
              <row at={[0, 0]} w={160} h={LINE} px={1} align="center" radius="sm" fill="well" border="focus">
                <field key="section-name" label={`Section at bar ${section.bar}`} value={section.name} focus style="label" w={150} onCommit={(text) => edits.marks.nameSection(text)} />
              </row>
            </box>
          );
        }
        return (
          <box place={[moved, 0, to - from, LINE]} clip>
            <box at={[0, 3]} w={1} h={LINE - 3} fill="border-strong" />
            <row key={key} label={`Section ${section.name}`} role="button" checked={chosen} at={[view.left.minus(from).max(0).times(view.zoom.times(ACROSS)).plus(4), 1]} h={LINE - 2} px={1} align="center" radius="sm"
              fill={chosen ? "selected" : undefined} border={chosen ? "focus" : undefined} hover="fill" drag={dragOf(from, next - start, 0)}
              onPress={pressOf(page, mark)} onDragEnd={(p) => edits.marks.moveMark(mark, p.dx)}>
              <text style="label" color="text" truncate>{section.name}</text>
            </row>
          </box>
        );
      })}
    </box>
  );
};

/** The time signature changes, while the song has more than one, in place of their bar numbers; and the tempo changes as ticks at the foot while the tempo lane is closed, their tempo shown under the pointer or chosen. */
const Changes = ({ page }: RulerProps) => {
  const { song, edits } = page;
  const { clock, memory } = song;
  const runs = clock.tempo_map.time_signatures;
  const changes = runs.length > 1 ? changeBars(runs) : [];
  const starts = barStarts(runs, page.end);
  return (
    <box>
      {changes.map((bar, index) => {
        const tick = starts[bar - 1] ?? 0;
        const signature = runs[index]!.signature;
        const width = widthOf(`${bar}  ${signature}`);
        const before = changes[index - 1];
        const shows = before === undefined ? 1 : fits(width + GAP, tick - starts[before - 1]!);
        const label = (
          <row gap={1} align="center">
            <text style="label" color="text-muted">{bar}</text>
            <row px={1} radius="sm" border={isChosen(page, "signature", tick) ? "text" : "border-strong"}><text style="label" color="text-secondary">{signature}</text></row>
          </row>
        );
        // Placed with no size, its label laid out in points from its bar.
        if (bar === 1) return <box place={[tick, NUMBERS - 1, 0, 0]}><box at={[0, 0]} w={width} h={18} px={1} opacity={shows} inert>{label}</box></box>;
        const mark: Mark = { kind: "signature", bar, tick };
        const key = `signature-${bar}`;
        const { start, end } = barAt(runs, tick);
        return (
          <box place={[live.drag(key).dx.plus(tick), NUMBERS - 1, 0, 0]}>
            <box key={key} label={`Time signature ${signature} at bar ${bar}`} role="button" checked={isChosen(page, "signature", tick)} at={[0, 0]} w={width} h={18} px={1} radius="sm"
              hover="fill" opacity={shows} drag={dragOf(tick, end - start, 1)} onPress={pressOf(page, mark)} onDragEnd={(p) => edits.marks.moveMark(mark, p.dx)}>
              {label}
            </box>
          </box>
        );
      })}
      {!memory.tempoLane && clock.tempo_map.tempo_changes.filter((change) => change.tick > 0).map((change) => {
        const bar = barNear(runs, change.tick).bar;
        const mark: Mark = { kind: "tempo", bar, tick: change.tick };
        const key = `tempo-${change.tick}`;
        const chosen = isChosen(page, "tempo", change.tick);
        const ink: Color = chosen ? "text" : "text-muted";
        const pill = (
          <row at={[10, 0]} h={18} px={1} align="center" radius="sm" fill="background" border="border-strong">
            <text style="label" color="text">{`${bpmText(change.bpm)} bpm`}</text>
          </row>
        );
        return (
          <box key={key} label={`Tempo change at bar ${bar}`} value={change.bpm} role="button" checked={chosen} hoverGroup place={[live.drag(key).dx.plus(change.tick), (LINE + RULER) / 2]} w={9} h={RULER - LINE}
            drag={dragOf(change.tick, memory.snap, 1)} onPress={pressOf(page, mark)} onDragEnd={(p) => edits.marks.moveMark(mark, p.dx)}>
            <box at={[4, RULER - LINE - 8]} w={1} h={8} fill={ink} />
            {chosen ? pill : <box reveal>{pill}</box>}
          </box>
        );
      })}
    </box>
  );
};

/**
 * The repeat's bar at the foot, once a drag over the ruler made one: bright while it repeats. A
 * press turns it on or off, its body drags it, its ends stretch it, on beats: none is a step of undo.
 */
const Repeat = ({ page }: RulerProps) => {
  const { memory } = page.song;
  const range = memory.repeatRange;
  if (!range) return null;
  const marks = page.edits.marks;
  const { from, to } = range;
  const [moving, starting, ending] = [live.drag("repeat"), live.drag("repeat-start"), live.drag("repeat-end")];
  const on = memory.repeating;
  const band: Color = on ? "text" : "text-muted";
  const toggle = () => marks.toggleRepeat();
  const step = (part: "move" | "start" | "end") => ({ increment: (by: number) => marks.moveRepeat(part, { dx: by * BEAT }), decrement: (by: number) => marks.moveRepeat(part, { dx: -by * BEAT }), toggle });
  return (
    <box key="repeat" label="Repeat" role="switch" checked={on} place={[moving.dx.plus(starting.dx).plus(from), RULER - 14, ending.dx.minus(starting.dx).plus(to - from), 14]}
      drag={{ x: [-from, 1e7], snap: [BEAT, 1], from: [from, 0] }} onPress={toggle} onDragEnd={(p) => marks.moveRepeat("move", p)} actions={step("move")}>
      <box inset={[0, 8, 0, 3]} radius="sm" fill={band} />
      <box key="repeat-start" label="Start of the repeat" role="slider" value={from} at={[0, 0]} w={8} h={14} drag={{ x: [-from, to - BEAT - from], snap: [BEAT, 1], from: [from, 0] }}
        onPress={toggle} onDragEnd={(p) => marks.moveRepeat("start", p)} actions={step("start")}>
        <box at={[0, 3]} w={2} h={5} fill={band} />
      </box>
      <box key="repeat-end" label="End of the repeat" role="slider" value={to} inset={[null, 0, 0, 0]} w={8} drag={{ x: [from + BEAT - to, 1e7], snap: [BEAT, 1], from: [to, 0] }}
        onPress={toggle} onDragEnd={(p) => marks.moveRepeat("end", p)} actions={step("end")}>
        <box inset={[null, 3, 0, null]} w={2} h={5} fill={band} />
      </box>
    </box>
  );
};
