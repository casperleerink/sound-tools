import { expect, test } from "bun:test";
import { editsOf } from "./edits";
import { clipOf, fakeSong, trackOf } from "./fake";
import { marksOf, shadedParts, withoutSignature, withSignature } from "./marks";
import { BAR, BEAT } from "./time";

test("the ruler's menu adds a tempo change at the tempo there, chosen with the playhead on it", () => {
  const f = fakeSong({ children: [] }, { piece: { tempo_map: { time_signatures: [], tempo_changes: [{ tick: 0, bpm: 90 }] } } });
  const { marks } = editsOf(f.song);
  const menu = marks.menuAt(2 * BAR + 200);
  expect(menu).toEqual({ tempo: 2 * BAR + 240, signature: 2 * BAR, section: 2 * BAR, removed: null });
  marks.addTempo(menu.tempo!);
  expect(f.piece.tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 90 }, { tick: 2 * BAR + 240, bpm: 90 }]);
  expect(f.memory.mark).toEqual({ kind: "tempo", bar: 3, tick: 2 * BAR + 240 });
  expect(f.clock.tick).toBe(2 * BAR + 240);
  expect(marks.menuAt(2 * BAR + 240).tempo).toBe(null);
  expect(f.steps).toEqual(["Add a tempo change"]);
});

test("a section added at the nearest bar line is named, and an empty name removes it", () => {
  const f = fakeSong({ children: [] });
  const { marks } = editsOf(f.song);
  marks.press({ x: BAR + 100, clicks: 2 });
  expect(f.piece.sections).toEqual([{ bar: 2, name: "Section 1" }]);
  expect(f.memory.renamingSection).toEqual({ bar: 2, tick: BAR });
  marks.nameSection("Verse");
  expect(f.piece.sections).toEqual([{ bar: 2, name: "Verse" }]);
  marks.pressMark({ kind: "section", bar: 2, tick: BAR }, { clicks: 2 });
  marks.nameSection("");
  expect(f.piece.sections).toEqual([]);
  expect(f.steps).toEqual(["Add a section", "Name a section", "Name a section"]);
});

test("a mark pressed is chosen and played from, and removed with its own words", () => {
  const f = fakeSong({ children: [] }, { piece: { sections: [{ bar: 3, name: "Chorus" }], tempo_map: { time_signatures: [], tempo_changes: [{ tick: 0, bpm: 120 }, { tick: BAR, bpm: 140 }] } } });
  const { marks } = editsOf(f.song);
  const tempo = marksOf(f.piece).find((mark) => mark.kind === "tempo")!;
  marks.pressMark(tempo, { clicks: 1 });
  expect([f.memory.mark, f.clock.tick]).toEqual([tempo, BAR]);
  marks.remove(tempo);
  marks.remove({ kind: "section", bar: 3, tick: 2 * BAR });
  expect(f.piece).toMatchObject({ sections: [], tempo_map: { tempo_changes: [{ tick: 0, bpm: 120 }] } });
  expect(f.memory.mark).toBe(null);
  expect(f.steps).toEqual(["Remove a tempo change", "Remove a section"]);
});

test("a time signature change splits its run so nothing moves, and its removal joins the bars to the run before", () => {
  const split = withSignature([{ signature: "3/4", bars: 8 }], 3);
  expect(split).toEqual([{ signature: "3/4", bars: 2 }, { signature: "3/4", bars: 6 }]);
  expect(withoutSignature(split, 3)).toEqual([{ signature: "3/4", bars: 8 }]);
  const f = fakeSong({ children: [] });
  expect(editsOf(f.song).marks.addSignature(BAR / 4)).toBe(false);
  expect(editsOf(f.song).marks.addSignature(BAR * 2 - 100)).toBe(true);
  expect([f.memory.mark, f.clock.tick]).toEqual([{ kind: "signature", bar: 3, tick: 2 * BAR }, 2 * BAR]);
});

test("alt-shift-right goes to the next mark and chooses it", () => {
  const f = fakeSong({ children: [] }, { tick: 100, piece: { sections: [{ bar: 2, name: "A" }, { bar: 5, name: "B" }] } });
  const { marks } = editsOf(f.song);
  expect(marks.step(1)).toBe(true);
  expect(f.clock.tick).toBe(BAR);
  marks.step(1);
  expect(f.memory.mark).toEqual({ kind: "section", bar: 5, tick: 4 * BAR });
  expect(marks.step(1)).toBe(false);
});

test("a drag on the ruler repeats a range on beats, which is no undo step; the bar turns it off and on, moves and stretches it", () => {
  const f = fakeSong({ children: [] });
  const { marks } = editsOf(f.song);
  marks.loop({ x: 2 * BEAT + 100, dx: -2000 });
  expect(f.clock.repeat).toEqual({ from: 0, to: 2 * BEAT });
  marks.toggleRepeat();
  expect([f.clock.repeat, f.memory.repeatRange]).toEqual([null, { from: 0, to: 2 * BEAT }]);
  marks.moveRepeat("move", { dx: BEAT });
  marks.toggleRepeat();
  expect(f.clock.repeat).toEqual({ from: BEAT, to: 3 * BEAT });
  marks.moveRepeat("end", { dx: -5 * BEAT });
  expect(f.clock.repeat).toEqual({ from: BEAT, to: 2 * BEAT });
  expect(f.steps).toEqual([]);
});

test("a mark dragged lands on the nearest bar line, or a tempo change on its tick, as one step that keeps it chosen; never onto another", () => {
  const f = fakeSong({ children: [] }, { piece: { sections: [{ bar: 2, name: "A" }, { bar: 5, name: "B" }], tempo_map: { time_signatures: [{ signature: "4/4", bars: 4 }, { signature: "3/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 120 }, { tick: BAR, bpm: 140 }] } } });
  const { marks } = editsOf(f.song);
  marks.pressMark({ kind: "section", bar: 2, tick: BAR }, { clicks: 1 });
  expect(marks.moveMark({ kind: "section", bar: 2, tick: BAR }, BAR + 300)).toBe(true);
  expect(f.piece.sections).toEqual([{ bar: 3, name: "A" }, { bar: 5, name: "B" }]);
  expect(f.memory.mark).toEqual({ kind: "section", bar: 3, tick: 2 * BAR });
  expect(marks.moveMark({ kind: "section", bar: 3, tick: 2 * BAR }, 2 * BAR)).toBe(false);
  marks.moveMark({ kind: "signature", bar: 5, tick: 4 * BAR }, -2 * BAR);
  expect(f.piece.tempo_map.time_signatures).toEqual([{ signature: "4/4", bars: 2 }, { signature: "3/4", bars: 3 }]);
  marks.moveMark({ kind: "tempo", bar: 2, tick: BAR }, BEAT, 150);
  expect(marks.moveMark({ kind: "tempo", bar: 1, tick: 0 }, BAR, 90)).toBe(true);
  expect(f.piece.tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 90 }, { tick: BAR + BEAT, bpm: 150 }]);
  expect(f.steps).toEqual(["Move a section", "Move a time signature change", "Move a tempo change", "Change the tempo"]);
});

test("L repeats what the chosen clips span, on beats; with none chosen it turns the repeat off and on", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("c00", BEAT + 100, BAR)])] });
  const { marks } = editsOf(f.song);
  expect(marks.repeatChosen()).toBe(false);
  f.memory.selected = ["t00/c00"];
  marks.repeatChosen();
  expect(f.clock.repeat).toEqual({ from: BEAT, to: BAR + 2 * BEAT });
  f.memory.selected = [];
  marks.repeatChosen();
  expect([f.clock.repeat, f.memory.repeatRange]).toEqual([null, { from: BEAT, to: BAR + 2 * BEAT }]);
  expect(f.steps).toEqual([]);
});

test("every other part of the song is shaded from the second, the bars before the first section a part", () => {
  const runs = [{ signature: "4/4", bars: 1 }];
  expect(shadedParts([], runs, 64 * BAR)).toEqual([]);
  expect(shadedParts([{ bar: 5, name: "Verse" }, { bar: 9, name: "Chorus" }, { bar: 13, name: "Bridge" }], runs, 64 * BAR)).toEqual([[4 * BAR, 8 * BAR], [12 * BAR, 64 * BAR]]);
  expect(shadedParts([{ bar: 1, name: "Intro" }, { bar: 3, name: "Verse" }], runs, 16 * BAR)).toEqual([[2 * BAR, 16 * BAR]]);
});
