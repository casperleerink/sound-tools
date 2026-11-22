import { expect, test } from "bun:test";
import { clipOf, fakeSong, regionOf, tapeOf, trackOf } from "../song/fake";
import { BAR, BEAT } from "../song/time";
import { scoreEdits, tempoMarks } from "./edits";
import { actions } from "./keys";

/** What the track `index` of the song holds at `at`, the fader first. */
const childOf = (self: unknown, index: number, at: number) => (self as { children: { children: unknown[] }[] }).children[index]?.children[at];

/** A song of one track with a clip of eight bars, at 100 bpm. */
const songOf = (piece = {}) => fakeSong({ children: [trackOf("t00", [clipOf("music", 0, 8 * BAR)])] }, { piece: { tempo_map: { time_signatures: [{ signature: "4/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 100 }] }, ...piece } });

test("a rit. over chosen bars is written once where it goes is typed, held from their end; typing it again moves what holds it", () => {
  const f = songOf();
  const edits = scoreEdits(f.song);
  edits.addGlide(3, 4);
  expect([f.memory.typing, f.steps]).toEqual([{ kind: "target", tick: 2 * BAR, end: 4 * BAR }, []]);
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

test("clearing where a rit. goes on a bar with a tempo mark and no rit. leaves the mark", () => {
  const f = songOf({ tempo_map: { time_signatures: [{ signature: "4/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 100 }, { tick: 4 * BAR, bpm: 72 }] } });
  const edits = scoreEdits(f.song);
  edits.addGlide(5);
  edits.typed(f.memory.typing!, "");
  expect([f.piece.tempo_map.tempo_changes, f.steps]).toEqual([[{ tick: 0, bpm: 100 }, { tick: 4 * BAR, bpm: 72 }], []]);
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

test("a bar added keeps the notes, the tempo map and the sections as one step, and the view where it is", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("music", 0, 4 * BAR, [{ start: 2 * BAR, length: BEAT, pitch: 60, velocity: 100 }])])] }, { piece: { sections: [{ bar: 3, name: "Theme" }] } });
  scoreEdits(f.song).addBars(2);
  expect(f.piece.sections).toEqual([{ bar: 4, name: "Theme" }]);
  expect(childOf(f.self, 0, 1)).toMatchObject({ state: { length: 5 * BAR, notes: [{ start: 3 * BAR }] } });
  expect(new Set(f.steps)).toEqual(new Set(["Add a bar"]));
  expect(f.shown).toEqual([]);
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

test("a bar added after the last keeps the last clip's end, so its last note does not ring on, and an empty clip holds the end line", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("music", 0, 4 * BAR, [{ start: 3 * BAR, length: 2 * BAR, pitch: 60, velocity: 100 }])])] });
  const edits = scoreEdits(f.song);
  edits.addBars(4, 1, true);
  expect(childOf(f.self, 0, 1)).toMatchObject({ state: { length: 4 * BAR } });
  expect(edits.end()).toBe(5 * BAR);
});

test("taking out a bar where the end line already moves makes no clip", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("music", 0, null, [{ start: 3 * BAR, length: BEAT, pitch: 60, velocity: 100 }])])] });
  const edits = scoreEdits(f.song);
  edits.removeBars(1, 1);
  expect((f.self as { children: { children: unknown[] }[] }).children[0]?.children.length).toBe(2);
  expect(edits.end()).toBe(3 * BAR);
});

test("audio that starts in a bar taken out starts where the bar was", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("music", 0, 4 * BAR)]), tapeOf("audio-1", [regionOf("voice", BAR + BEAT, 1)])] });
  scoreEdits(f.song).removeBars(2, 2);
  expect(childOf(f.self, 1, 1)).toMatchObject({ state: { start: BAR } });
});

test("a piece of audio and no clip gains a bar at its end", () => {
  const f = fakeSong({ children: [trackOf("t00"), tapeOf("audio-1", [regionOf("voice", 0, 8)])] });
  const edits = scoreEdits(f.song);
  expect(edits.end()).toBe(4 * BAR);
  edits.addBars(4, 1, true);
  expect(edits.end()).toBe(5 * BAR);
});

test("taking out the bar of the last clip moves the end line one bar, and no note before it plays longer", () => {
  const held = { start: 0, length: 2 * BAR, pitch: 60, velocity: 100 };
  const f = fakeSong({ children: [trackOf("t00", [clipOf("first", 0, BAR, [held]), clipOf("last", 2 * BAR, BAR)])] });
  const edits = scoreEdits(f.song);
  edits.removeBars(3, 3);
  expect(edits.end()).toBe(2 * BAR);
  expect(childOf(f.self, 0, 1)).toMatchObject({ state: { length: BAR } });
});

test("a press after a clip that is as long as its notes opens what is there, not that clip", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("short", 0, null, [{ start: 0, length: BEAT, pitch: 60, velocity: 100 }]), clipOf("long", 4 * BAR, 4 * BAR)])] });
  scoreEdits(f.song).openMusic("t00", 4 * BAR + BEAT);
  expect(f.memory.below).toEqual({ kind: "editor", clip: "t00/long" });
});

test("a piece with no music still gains and loses bars, and a press past its end makes no clip", () => {
  const f = fakeSong({ children: [trackOf("t00")] });
  const edits = scoreEdits(f.song);
  edits.addBars(17);
  expect(edits.end()).toBe(17 * BAR);
  edits.removeBars(1, 2);
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

test("chosen bars across time signatures come out as one step, all after them moving earlier, and the choice goes", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("music", 0, 6 * BAR, [{ start: 4 * BAR, length: BEAT, pitch: 60, velocity: 100 }])])] }, { piece: { tempo_map: { time_signatures: [{ signature: "4/4", bars: 2 }, { signature: "3/4", bars: 2 }, { signature: "4/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 100 }] } } });
  const edits = scoreEdits(f.song);
  edits.choose(2);
  edits.choose(3, true);
  expect(f.memory.bars).toEqual({ from: 2, to: 3 });
  expect(edits.removeBars(2, 3)).toBe(true);
  expect(f.piece.tempo_map.time_signatures).toEqual([{ signature: "4/4", bars: 1 }, { signature: "3/4", bars: 1 }, { signature: "4/4", bars: 1 }]);
  // Bar 2 of 4/4 and bar 3 of 3/4 are a bar and three beats.
  expect(childOf(f.self, 0, 1)).toMatchObject({ state: { notes: [{ start: 4 * BAR - BAR - 3 * BEAT }] } });
  expect([new Set(f.steps), f.memory.bars]).toEqual([new Set(["Remove bars"]), null]);
  // A bar stays.
  expect(edits.removeBars(1, 99)).toBe(false);
});

test("a bar added before the chosen bars moves the choice with them", () => {
  const f = songOf();
  const edits = scoreEdits(f.song);
  edits.chooseAcross(4, 3);
  edits.addBars(3);
  expect(f.memory.bars).toEqual({ from: 4, to: 5 });
  edits.addBars(5, 1, true);
  expect(f.memory.bars).toEqual({ from: 4, to: 5 });
});

test("Delete clears the notes of the chosen bars and cuts one ringing into them; the bars stay", () => {
  const note = (start: number, length = BEAT) => ({ start, length, pitch: 60, velocity: 100 });
  const f = fakeSong({ children: [trackOf("t00", [clipOf("music", 0, 4 * BAR, [note(BAR - BEAT, 2 * BEAT), note(BAR + BEAT), note(2 * BAR)])])] });
  // With no bar chosen, and with notes chosen in the editor, the key is not theirs.
  expect(actions.clear_bars!.run(f.song)).toBe(false);
  scoreEdits(f.song).choose(2);
  f.memory.notes = ["0"];
  expect(actions.clear_bars!.run(f.song)).toBe(false);
  f.memory.notes = [];
  actions.clear_bars!.run(f.song);
  expect(childOf(f.self, 0, 1)).toMatchObject({ state: { length: 4 * BAR, notes: [note(BAR - BEAT), note(2 * BAR)] } });
  expect(f.steps).toEqual(["Clear the notes"]);
});
