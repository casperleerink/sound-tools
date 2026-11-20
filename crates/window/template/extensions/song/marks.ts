// The marks of the ruler: sections, time signature changes and tempo changes, and the range the
// song repeats. Marks are in `project.json`, so each change is one undo step of the clock; the
// repeat is not saved and is no undo step. Its range and whether it repeats are kept in memory
// too: a view that read the clock's would run again at every play and pause. The ruler's plane: x in ticks of the song.
import type { Pointer, Section, TempoMap } from "../sdk";
import { lengthAt } from "./clips";
import { itemAt, repeatOf, type Mark, type Song } from "./model";
import { barNear, barStarts, BEAT, nearestOf, tickOfBar, type Runs } from "./time";

/** What the marks are read from: the clock, or a test's copy of `project.json`. */
type Piece = { tempo_map: { time_signatures: Runs; tempo_changes: readonly { tick: number; bpm: number }[] }; sections: readonly Section[] };

/** The bar each run starts at, from 1: where a time signature change is, even to the same one. */
export function changeBars(runs: Runs): number[] {
  const bars = [1];
  for (const run of runs.slice(0, -1)) bars.push(bars.at(-1)! + run.bars);
  return bars;
}

/** Every mark of the ruler, in time: the sections, the time signature changes and the tempo changes, each after the first. */
export function marksOf({ tempo_map, sections }: Piece): Mark[] {
  const runs = tempo_map.time_signatures;
  const named = sections.map(({ bar }) => ({ kind: "section" as const, bar, tick: tickOfBar(runs, bar) }));
  const signatures = changeBars(runs).slice(1).map((bar) => ({ kind: "signature" as const, bar, tick: tickOfBar(runs, bar) }));
  const tempos = tempo_map.tempo_changes.filter((change) => change.tick > 0).map(({ tick }) => ({ kind: "tempo" as const, bar: barNear(runs, tick).bar, tick }));
  return [...named, ...signatures, ...tempos].sort((a, b) => a.tick - b.tick);
}

/** The parts of the song every other of which is a shade lighter, from the second: [from, to] ticks, the bars before the first section a part too. */
export function shadedParts(sections: readonly Section[], runs: Runs, end: number): [number, number][] {
  const starts = barStarts(runs, end);
  const edges = sections.map((section) => starts[section.bar - 1] ?? end);
  if (edges[0] !== 0) edges.unshift(0);
  edges.push(end);
  return edges.slice(0, -1).flatMap((from, index) => (index % 2 === 1 ? [[from, edges[index + 1]!] as [number, number]] : []));
}

/** The time signatures with a change at bar `bar` to the one that plays there: the run it is in, cut in two, so nothing moves. */
export function withSignature(runs: Runs, bar: number): TempoMap["time_signatures"] {
  const all = runs.length > 0 ? runs : [{ signature: "4/4", bars: 1 }];
  const firsts = changeBars(all);
  if (firsts.includes(bar)) return all.map((run) => ({ ...run }));
  const index = firsts.findLastIndex((first) => first <= bar);
  const [run, first] = [all[index]!, firsts[index]!];
  return [
    ...all.slice(0, index).map((each) => ({ ...each })),
    { signature: run.signature, bars: bar - first },
    // The last run goes on, whatever its count says.
    { signature: run.signature, bars: Math.max(run.bars - (bar - first), 1) },
    ...all.slice(index + 1).map((each) => ({ ...each })),
  ];
}

/** The time signatures without the change at bar `bar`: its bars join the run before. */
export function withoutSignature(runs: Runs, bar: number): TempoMap["time_signatures"] {
  const index = changeBars(runs).indexOf(bar);
  const [before, run] = [runs[index - 1], runs[index]];
  if (!before || !run) return runs.map((each) => ({ ...each }));
  return [...runs.slice(0, index - 1), { ...before, bars: before.bars + run.bars }, ...runs.slice(index + 1)].map((each) => ({ ...each }));
}

/** The time signatures with the change at bar `from` moved to bar `to`, the bars between going to the run before. */
export function withSignatureMoved(runs: Runs, from: number, to: number): TempoMap["time_signatures"] {
  const signature = runs[changeBars(runs).indexOf(from)]?.signature ?? "4/4";
  const moved = withSignature(withoutSignature(runs, from), to);
  return moved.map((run, index) => (changeBars(moved)[index] === to ? { ...run, signature } : run));
}

/** The slowest and the fastest tempo, as the transport takes them. */
export const [SLOWEST, FASTEST] = [10, 1000];

/** The tempo that plays at `tick`. */
const bpmAt = (changes: readonly { tick: number; bpm: number }[], tick: number) => [...changes].reverse().find((change) => change.tick <= tick)?.bpm ?? 120;

/** What the ruler does: each change of the marks one undo step; the repeat none. */
export const markEdits = (song: Song) => {
  const { clock, memory } = song;
  const runs = () => clock.tempo_map.time_signatures;
  /** Repeats `range` or keeps it to show, off. */
  const repeat = (range: { from: number; to: number }, on: boolean) => {
    memory.repeatRange = range;
    clock.setRepeat(on ? range : null);
  };
  const choose = (mark: Mark) => {
    memory.selected = [];
    memory.point = null;
    memory.mark = mark;
  };
  /** A tempo change at `tick` with the tempo that plays there, chosen; one there already is chosen. */
  const tempoAt = (tick: number) => {
    const changes = clock.tempo_map.tempo_changes;
    if (!changes.some((change) => change.tick === tick)) {
      clock.update("Add a tempo change", (piece) => (piece.tempo_map.tempo_changes = [...piece.tempo_map.tempo_changes, { tick, bpm: bpmAt(changes, tick) }].sort((a, b) => a.tick - b.tick)));
    }
    const starts = barStarts(runs(), tick + 1);
    choose({ kind: "tempo", bar: Math.max(starts.findIndex((start) => start > tick), 1), tick });
  };
  const edits = {
    /** A section at the bar line nearest `tick`, named by how many there are; its name is then typed. */
    addSection(tick: number) {
      const { bar, tick: line } = barNear(runs(), tick);
      if (!clock.sections.some((each) => each.bar === bar)) {
        clock.update("Add a section", (piece) => {
          piece.sections = [...piece.sections, { bar, name: `Section ${piece.sections.length + 1}` }].sort((a, b) => a.bar - b.bar);
        });
      }
      memory.renamingSection = { bar, tick: line };
    },
    /** A tempo change at `tick`, from the ruler's menu, chosen with the playhead on it, for the transport to change. */
    addTempo(tick: number) {
      tempoAt(tick);
      clock.jump(tick);
    },
    /** A time signature change at the bar line nearest `tick`, chosen with the playhead on it: none at bar 1. */
    addSignature(tick: number): boolean {
      const { bar, tick: line } = barNear(runs(), tick);
      if (bar === 1) return false;
      if (!changeBars(runs()).includes(bar)) clock.update("Add a time signature change", (piece) => (piece.tempo_map.time_signatures = withSignature(piece.tempo_map.time_signatures, bar)));
      choose({ kind: "signature", bar, tick: line });
      clock.jump(line);
      return true;
    },
    /** T: a tempo change at the playhead; while it plays, on the snap. */
    addTempoAtPlayhead() {
      tempoAt(clock.playing ? nearestOf(clock.tick, memory.snap) : Math.round(clock.tick));
    },
    /** Removes a mark as one undo step with its own words. */
    remove(mark: Mark) {
      if (memory.mark?.kind === mark.kind && memory.mark.tick === mark.tick) memory.mark = null;
      if (mark.kind === "section") clock.update("Remove a section", (piece) => (piece.sections = piece.sections.filter((each) => each.bar !== mark.bar)));
      else if (mark.kind === "tempo") clock.update("Remove a tempo change", (piece) => (piece.tempo_map.tempo_changes = piece.tempo_map.tempo_changes.filter((each) => each.tick !== mark.tick)));
      else clock.update("Remove a time signature change", (piece) => (piece.tempo_map.time_signatures = withoutSignature(piece.tempo_map.time_signatures, mark.bar)));
    },
    /** The name typed for the section being named: an empty name removes it. */
    nameSection(text: string | null) {
      const named = memory.renamingSection;
      memory.renamingSection = null;
      if (!named || text === null) return;
      clock.update("Name a section", (piece) => {
        const rest = piece.sections.filter((each) => each.bar !== named.bar);
        piece.sections = text.trim() === "" ? rest : [...rest, { bar: named.bar, name: text.trim() }].sort((a, b) => a.bar - b.bar);
      });
    },
    /** A press on the ruler at `x`: it plays from the snap there; a double click adds a section at the nearest bar line. */
    press(p: Pick<Pointer, "x" | "clicks">) {
      memory.mark = null;
      if (p.clicks === 2) edits.addSection(p.x);
      else clock.jump(nearestOf(p.x, memory.snap));
    },
    /** A press on a mark: chosen and played from; a double click on a section names it. */
    pressMark(mark: Mark, p: Pick<Pointer, "clicks">) {
      choose(mark);
      clock.jump(mark.tick);
      if (p.clicks === 2 && mark.kind === "section") memory.renamingSection = { bar: mark.bar, tick: mark.tick };
    },
    /**
     * What the ruler's menu at `x` offers: a tempo change on the snap, a time signature or a
     * section on the nearest bar line, each `null` where one is already; and the mark there to remove.
     */
    menuAt(x: number, mark?: Mark) {
      const [tick, { bar, tick: line }] = [nearestOf(x, memory.snap), barNear(runs(), x)];
      const section = clock.sections.find((each) => each.bar === bar);
      const signature = bar > 1 && changeBars(runs()).includes(bar);
      const tempo = clock.tempo_map.tempo_changes.find((each) => each.tick > 0 && (each.tick === tick || each.tick === line));
      const removed: Mark | null = mark ?? (section ? { kind: "section", bar, tick: line } : signature ? { kind: "signature", bar, tick: line } : tempo ? { kind: "tempo", bar, tick: tempo.tick } : null);
      return {
        tempo: clock.tempo_map.tempo_changes.some((each) => each.tick === tick) ? null : tick,
        signature: bar === 1 || signature ? null : line,
        section: section ? null : line,
        removed,
      };
    },
    /** Goes to the mark after or before the chosen one, or the playhead, and chooses it. */
    step(by: -1 | 1): boolean {
      const marks = marksOf(clock);
      const chosen = memory.mark;
      const at = chosen ? marks.findIndex((mark) => mark.kind === chosen.kind && mark.tick === chosen.tick) : -1;
      const next = at >= 0 ? marks[at + by] : by > 0 ? marks.find((mark) => mark.tick >= clock.tick) : marks.findLast((mark) => mark.tick <= clock.tick);
      if (!next) return false;
      choose(next);
      clock.jump(next.tick);
      return true;
    },
    /**
     * A drag of a mark by `dx`: a section or a time signature change to the bar line nearest
     * where it lands, a tempo change to that tick, at `bpm` when given. None onto another of its
     * kind, and none moves the first tempo or time signature: one undo step, the mark kept chosen.
     */
    moveMark(mark: Mark, dx: number, bpm?: number): boolean {
      const { bar, tick: line } = barNear(runs(), Math.max(mark.tick + dx, 0));
      let moved: Mark | null = null;
      if (mark.kind === "section" && bar !== mark.bar && !clock.sections.some((each) => each.bar === bar)) {
        clock.update("Move a section", (piece) => {
          piece.sections = piece.sections.map((each) => (each.bar === mark.bar ? { ...each, bar } : each)).sort((a, b) => a.bar - b.bar);
        });
        moved = { kind: "section", bar, tick: line };
      } else if (mark.kind === "signature" && bar > 1 && bar !== mark.bar && !changeBars(runs()).includes(bar)) {
        clock.update("Move a time signature change", (piece) => (piece.tempo_map.time_signatures = withSignatureMoved(piece.tempo_map.time_signatures, mark.bar, bar)));
        moved = { kind: "signature", bar, tick: line };
      } else if (mark.kind === "tempo") {
        const to = mark.tick === 0 ? 0 : Math.max(Math.round(mark.tick + dx), 1);
        const changes = clock.tempo_map.tempo_changes;
        const was = changes.find((change) => change.tick === mark.tick);
        const speed = Math.min(Math.max(bpm ?? was?.bpm ?? 120, SLOWEST), FASTEST);
        if (!was || (to !== mark.tick && changes.some((change) => change.tick === to)) || (to === mark.tick && speed === was.bpm)) return false;
        clock.update(to === mark.tick ? "Change the tempo" : "Move a tempo change", (piece) => {
          const rest = piece.tempo_map.tempo_changes.filter((change) => change.tick !== mark.tick);
          piece.tempo_map.tempo_changes = [...rest, { tick: to, bpm: speed }].sort((a, b) => a.tick - b.tick);
        });
        moved = { kind: "tempo", bar: barNear(runs(), to).bar, tick: to };
      }
      if (!moved) return false;
      if (memory.mark?.kind === mark.kind && memory.mark.tick === mark.tick) memory.mark = moved;
      return true;
    },
    /** Repeats what the chosen clips and regions span, on beats, and turns the repeat on; with none chosen, turns it on or off. */
    repeatChosen(): boolean {
      const spans = memory.selected.flatMap((key) => {
        const start = itemAt(song.children, key)?.state.start;
        return start === undefined ? [] : [{ from: start, to: start + lengthAt(song, key) }];
      });
      if (spans.length === 0) {
        return edits.toggleRepeat();
      }
      const from = Math.floor(Math.min(...spans.map((each) => each.from)) / BEAT) * BEAT;
      const to = Math.ceil(Math.max(...spans.map((each) => each.to)) / BEAT) * BEAT;
      repeat({ from, to: Math.max(to, from + BEAT) }, true);
      return true;
    },
    /** A drag across the ruler from `x`: it repeats that range, on beats. */
    loop(p: Pick<Pointer, "x" | "dx">) {
      const [from, to] = [Math.max(Math.round(Math.min(p.x, p.x + p.dx) / BEAT) * BEAT, 0), Math.round(Math.max(p.x, p.x + p.dx) / BEAT) * BEAT];
      if (to > from) repeat({ from, to }, true);
    },
    /** A click on the repeat's bar: it repeats, or plays on and keeps its range. */
    toggleRepeat(): boolean {
      const { range, on } = repeatOf(song);
      if (!range) return false;
      repeat(range, !on);
      return true;
    },
    /** A drag of the repeat's bar by `dx` on beats: its body moves it, an end stretches it, at least a beat. */
    moveRepeat(part: "move" | "start" | "end", p: Pick<Pointer, "dx">) {
      const { range, on } = repeatOf(song);
      if (!range) return;
      const { from, to } = range;
      const moved = Math.max(from + p.dx, 0) - from;
      const [start, end] = part === "move" ? [from + moved, to + moved] : part === "start" ? [Math.min(Math.max(from + p.dx, 0), to - BEAT), to] : [from, Math.max(to + p.dx, from + BEAT)];
      repeat({ from: start, to: end }, on);
    },
  };
  return edits;
};
