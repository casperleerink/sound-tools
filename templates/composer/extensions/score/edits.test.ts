import { expect, test } from "bun:test";
import { clipOf, fakeSong, trackOf } from "../song/fake";
import { BAR, BEAT } from "../song/time";
import type { Item } from "./bars";
import { scoreEdits, tempoMarks } from "./edits";

/** What the track `index` of the song holds at `at`, the fader first. */
const childOf = (self: unknown, index: number, at: number) => (self as { children: Item[] }).children[index]?.children?.[at];

/** A song of one track with a clip of eight bars, at 100 bpm. */
const songOf = (piece = {}) => fakeSong({ children: [trackOf("t00", [clipOf("music", 0, 8 * BAR)])] }, { piece: { tempo_map: { time_signatures: [{ signature: "4/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 100 }] }, ...piece } });

test("a rit. goes to three quarters of the tempo and holds there two bars on; typing where it goes moves what holds it", () => {
  const f = songOf();
  const edits = scoreEdits(f.song);
  edits.addGlide(3);
  expect(f.piece.tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 100 }, { tick: 2 * BAR, bpm: 100, to: 75 }, { tick: 4 * BAR, bpm: 75 }]);
  expect(f.memory.typing).toEqual({ kind: "target", tick: 2 * BAR });
  edits.typed("120");
  expect(f.piece.tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 100 }, { tick: 2 * BAR, bpm: 100, to: 120 }, { tick: 4 * BAR, bpm: 120 }]);
  // Where it goes above where it starts makes it an accel.; the mark that held it says nothing new.
  expect(tempoMarks(f.piece.tempo_map.tempo_changes).map((mark) => mark.shows)).toEqual([true, false, false]);
  expect(f.steps).toEqual(["Add a rit.", "Change where the tempo goes"]);
});

test("typing nothing where a rit. goes takes it out, and the mark that held where it went", () => {
  const f = songOf();
  const edits = scoreEdits(f.song);
  edits.addGlide(3);
  edits.typed("");
  expect(f.piece.tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 100 }]);
});

test("a tempo typed on a mark changes it, and the tempo that already plays takes the mark out", () => {
  const f = songOf();
  const edits = scoreEdits(f.song);
  edits.addTempo(5);
  expect(f.memory.typing).toEqual({ kind: "tempo", tick: 4 * BAR });
  edits.typed("72");
  expect(f.piece.tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 100 }, { tick: 4 * BAR, bpm: 72 }]);
  f.memory.typing = { kind: "tempo", tick: 4 * BAR };
  edits.typed("100");
  expect(f.piece.tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 100 }]);
});

test("a time signature typed at a bar holds to the next change, and typing the one before it takes the change out", () => {
  const f = songOf({ tempo_map: { time_signatures: [{ signature: "4/4", bars: 4 }, { signature: "6/8", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 100 }] } });
  const edits = scoreEdits(f.song);
  edits.signature(3, "3/4");
  expect(f.piece.tempo_map.time_signatures).toEqual([{ signature: "4/4", bars: 2 }, { signature: "3/4", bars: 2 }, { signature: "6/8", bars: 1 }]);
  edits.signature(3, "4/4");
  expect(f.piece.tempo_map.time_signatures).toEqual([{ signature: "4/4", bars: 4 }, { signature: "6/8", bars: 1 }]);
  edits.signature(5, "5 / 9");
  expect(f.steps).toEqual(["Change the time signature", "Change the time signature"]);
});

test("a fermata goes over the last beat of its bar, in the beat of its time signature, and a second press takes it out", () => {
  const f = songOf({ tempo_map: { time_signatures: [{ signature: "4/4", bars: 1 }, { signature: "6/8", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 100 }] } });
  const edits = scoreEdits(f.song);
  edits.toggleFermata(2);
  expect(f.piece.tempo_map.fermatas).toEqual([{ tick: BAR + 3 * BEAT - BEAT / 2, hold: 2 }]);
  edits.toggleFermata(2);
  expect(f.piece.tempo_map.fermatas).toEqual([]);
});

test("a bar added keeps the notes, the tempo map and the sections as one step, and shows the whole piece again", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("music", 0, 4 * BAR, [{ start: 2 * BAR, length: BEAT, pitch: 60, velocity: 100 }])])] }, { piece: { sections: [{ bar: 3, name: "Theme" }] } });
  scoreEdits(f.song).addBars(2);
  expect(f.piece.sections).toEqual([{ bar: 4, name: "Theme" }]);
  expect(childOf(f.self, 0, 1)).toMatchObject({ state: { length: 5 * BAR, notes: [{ start: 3 * BAR }] } });
  expect(new Set(f.steps)).toEqual(new Set(["Add a bar"]));
  expect(f.shown).toEqual([["song", { x: [0, 5 * BAR + BAR / 2] }]]);
});

test("a press on a staff opens the notes there, in a new clip from that bar to the end when there is none", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("music", 0, 4 * BAR)]), trackOf("t01")] });
  const edits = scoreEdits(f.song);
  edits.openMusic("t00", 2 * BAR + 100);
  expect(f.memory.below).toEqual({ kind: "editor", clip: "t00/music" });
  edits.openMusic("t01", 2 * BAR + 100);
  expect(f.memory.below).toEqual({ kind: "editor", clip: "t01/clip" });
  expect(childOf(f.self, 1, 1)).toMatchObject({ state: { start: 0, length: 4 * BAR } });
});
