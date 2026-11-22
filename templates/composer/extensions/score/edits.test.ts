import { expect, test } from "bun:test";
import { clipOf, fakeSong, regionOf, tapeOf, trackOf } from "../song/fake";
import { BAR, BEAT } from "../song/time";
import { scoreEdits, tempoMarks } from "./edits";

/** What the track `index` of the song holds at `at`, the fader first. */
const childOf = (self: unknown, index: number, at: number) => (self as { children: { children: unknown[] }[] }).children[index]?.children[at];

/** A song of one track with a clip of eight bars, at 100 bpm. */
const songOf = (piece = {}) => fakeSong({ children: [trackOf("t00", [clipOf("music", 0, 8 * BAR)])] }, { piece: { tempo_map: { time_signatures: [{ signature: "4/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 100 }] }, ...piece } });

test("a rit. is written once where it goes is typed, held two bars on; typing it again moves what holds it", () => {
  const f = songOf();
  const edits = scoreEdits(f.song);
  edits.addGlide(3);
  expect([f.memory.typing, f.steps]).toEqual([{ kind: "target", tick: 2 * BAR }, []]);
  edits.typed(f.memory.typing!, "75");
  expect(f.piece.tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 100 }, { tick: 2 * BAR, bpm: 100, to: 75 }, { tick: 4 * BAR, bpm: 75 }]);
  f.memory.typing = { kind: "target", tick: 2 * BAR };
  edits.typed(f.memory.typing!, "120");
  expect(f.piece.tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 100 }, { tick: 2 * BAR, bpm: 100, to: 120 }, { tick: 4 * BAR, bpm: 120 }]);
  // Where it goes above where it starts makes it an accel.; the mark that held it says nothing new.
  expect(tempoMarks(f.piece.tempo_map.tempo_changes).map((mark) => mark.shows)).toEqual([true, false, false]);
  expect(f.steps).toEqual(["Add a rit.", "Change where the tempo goes"]);
});

test("typing nothing where a rit. goes takes it out, and the mark that held where it went", () => {
  const f = songOf();
  const edits = scoreEdits(f.song);
  edits.addGlide(3);
  edits.typed(f.memory.typing!, "75");
  f.memory.typing = { kind: "target", tick: 2 * BAR };
  edits.typed(f.memory.typing!, "");
  expect(f.piece.tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 100 }]);
});

test("a tempo mark is one step when typed, Escape adds nothing, and the tempo that already plays takes it out", () => {
  const f = songOf();
  const edits = scoreEdits(f.song);
  edits.addTempo(6);
  edits.typed(f.memory.typing!, null);
  edits.addTempo(5);
  expect(f.memory.typing).toEqual({ kind: "tempo", tick: 4 * BAR });
  edits.typed(f.memory.typing!, "72");
  expect(f.piece.tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 100 }, { tick: 4 * BAR, bpm: 72 }]);
  expect(f.steps).toEqual(["Add a tempo mark"]);
  f.memory.typing = { kind: "tempo", tick: 4 * BAR };
  edits.typed(f.memory.typing!, "100");
  expect(f.piece.tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 100 }]);
});

test("a tempo mark typed inside a rit. ends the rit. on its tempo, with no jump", () => {
  const f = songOf({ tempo_map: { time_signatures: [{ signature: "4/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 120, to: 60 }, { tick: 4 * BAR, bpm: 60 }] } });
  const edits = scoreEdits(f.song);
  edits.addTempo(3);
  edits.typed(f.memory.typing!, "90");
  expect(f.piece.tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 120, to: 90 }, { tick: 2 * BAR, bpm: 90 }, { tick: 4 * BAR, bpm: 60 }]);
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

test("a fermata's menu takes out that fermata, also one a time signature change left off its bar's last beat", () => {
  const f = songOf();
  const edits = scoreEdits(f.song);
  edits.toggleFermata(2);
  edits.signature(2, "3/4");
  edits.removeFermata(BAR + 3 * BEAT);
  expect(f.piece.tempo_map.fermatas).toEqual([]);
});

test("a value typed in one field lands on its own mark, whichever field is open", () => {
  const f = songOf();
  const edits = scoreEdits(f.song);
  edits.addTempo(9);
  edits.typed({ kind: "tempo", tick: 4 * BAR }, "72");
  expect(f.piece.tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 100 }, { tick: 4 * BAR, bpm: 72 }]);
  expect(f.memory.typing).toEqual({ kind: "tempo", tick: 8 * BAR });
});

test("a bar added keeps the notes, the tempo map and the sections as one step, and shows the whole piece again", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("music", 0, 4 * BAR, [{ start: 2 * BAR, length: BEAT, pitch: 60, velocity: 100 }])])] }, { piece: { sections: [{ bar: 3, name: "Theme" }] } });
  scoreEdits(f.song).addBars(2);
  expect(f.piece.sections).toEqual([{ bar: 4, name: "Theme" }]);
  expect(childOf(f.self, 0, 1)).toMatchObject({ state: { length: 5 * BAR, notes: [{ start: 3 * BAR }] } });
  expect(new Set(f.steps)).toEqual(new Set(["Add a bar"]));
  expect(f.shown).toEqual([["song", { x: [0, 5 * BAR + BAR / 2] }]]);
});

test("a bar added after the last bar of a run is in its time signature", () => {
  const f = songOf({ tempo_map: { time_signatures: [{ signature: "4/4", bars: 2 }, { signature: "3/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 100 }] } });
  scoreEdits(f.song).addBars(2, 1, true);
  expect(f.piece.tempo_map.time_signatures).toEqual([{ signature: "4/4", bars: 3 }, { signature: "3/4", bars: 1 }]);
});

test("a bar added at the end line makes the piece longer, also when its music ends inside the last bar", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("music", 0, 3 * BAR + BEAT)])] });
  const edits = scoreEdits(f.song);
  expect(edits.end()).toBe(4 * BAR);
  edits.addBars(4, 1, true);
  expect(edits.end()).toBe(5 * BAR);
});

test("audio that starts in a bar taken out starts where the bar was", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("music", 0, 4 * BAR)]), tapeOf("audio-1", [regionOf("voice", BAR + BEAT, 1)])] });
  scoreEdits(f.song).removeBar(2);
  expect(childOf(f.self, 1, 1)).toMatchObject({ state: { start: BAR } });
});

test("a piece of audio and no clip gains a bar at its end", () => {
  const f = fakeSong({ children: [trackOf("t00"), tapeOf("audio-1", [regionOf("voice", 0, 8)])] });
  const edits = scoreEdits(f.song);
  expect(edits.end()).toBe(4 * BAR);
  edits.addBars(4, 1, true);
  expect(edits.end()).toBe(5 * BAR);
});

test("a piece with no music still gains and loses bars, and a press past its end makes no clip", () => {
  const f = fakeSong({ children: [trackOf("t00")] });
  const edits = scoreEdits(f.song);
  edits.addBars(17);
  expect(edits.end()).toBe(17 * BAR);
  edits.removeBar(1);
  edits.removeBar(1);
  expect(edits.end()).toBe(15 * BAR);
  edits.openMusic("t00", 15 * BAR + BEAT);
  expect(f.memory.below).toBe(null);
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
