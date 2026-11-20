import { expect, test } from "bun:test";
import { editsOf } from "./edits";
import { clipOf, fakeSong, trackOf } from "./fake";
import { lengthText, noteCommand, pitchName, pitchOf } from "./notes";
import { BAR, BEAT } from "./time";

const note = (start: number, pitch = 60, velocity = 100, length = BEAT) => ({ start, length, pitch, velocity });
/** A song with the editor open on `t00/c00`, `length` long, holding `notes`. */
const open = (notes: ReturnType<typeof note>[], length: number | null = BAR, tick = 0) => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("c00", BAR, length, notes)])] }, { tick });
  f.memory.below = { kind: "editor", clip: "t00/c00" };
  return Object.assign(f, { edits: editsOf(f.song).notes("t00/c00"), notes: () => (f.self.children[0] as any).children[1].state.notes as ReturnType<typeof note>[] });
};

test("a double click adds a note on the grid, chosen, and a double click on it removes it", () => {
  const f = open([]);
  f.edits.pressEmpty({ x: 250, y: 127 - 64, clicks: 2, shift: false, cmd: false });
  expect(f.notes()).toEqual([note(240, 64, 100, BEAT / 2)]);
  expect(f.memory.notes.length).toBe(1);
  f.edits.remove(f.notes()[0]!);
  expect(f.notes()).toEqual([]);
  expect(f.steps).toEqual(["Add a note", "Delete notes"]);
});

test("a drag moves the chosen notes as one step, and with alt copies them, the copies chosen", () => {
  const f = open([note(0), note(BEAT, 62)]);
  f.edits.press(f.notes()[0]!, { shift: false, cmd: false, right: false });
  f.edits.move({ dx: BEAT / 2, dy: -2, alt: false });
  expect(f.notes()).toEqual([note(BEAT / 2, 62), note(BEAT, 62)]);
  f.edits.move({ dx: BEAT, dy: 0, alt: true });
  expect(f.notes().length).toBe(3);
  expect(f.memory.notes).toEqual([`${BEAT * 1.5}:62:${BEAT}:100`]);
  expect(f.steps).toEqual(["Move notes", "Copy notes"]);
});

test("a drag of the end resizes the chosen notes, at least 1/64, and new notes take that length", () => {
  const f = open([note(0)]);
  f.memory.notes = ["0:60:960:100"];
  f.edits.resize({ dx: -2000 });
  expect(f.notes()[0]!.length).toBe(BEAT / 16);
  expect(f.memory.length).toBe(BEAT / 16);
});

test("a marquee chooses the notes it touches, and delete takes them out", () => {
  const f = open([note(0, 60), note(BEAT, 72), note(2 * BEAT, 60)]);
  f.edits.marquee({ x: 100, y: 127 - 60, dx: BEAT, dy: 0.5, shift: false, cmd: false });
  expect(f.memory.notes).toEqual(["0:60:960:100"]);
  expect(noteCommand(f.song, "delete")).toBe(true);
  expect(f.notes()).toEqual([note(BEAT, 72), note(2 * BEAT, 60)]);
});

test("copy, paste after the chosen, duplicate right after, and choose all work from the keys", () => {
  const f = open([note(0), note(BEAT)], 2 * BAR);
  noteCommand(f.song, "all");
  noteCommand(f.song, "copy");
  noteCommand(f.song, "paste");
  expect(f.notes().map((each) => each.start)).toEqual([0, BEAT, 2 * BEAT, 3 * BEAT]);
  noteCommand(f.song, "duplicate");
  expect(f.notes().map((each) => each.start)).toEqual([0, BEAT, 2 * BEAT, 3 * BEAT, 4 * BEAT, 5 * BEAT]);
  expect(f.steps).toEqual(["Paste notes", "Duplicate notes"]);
});

test("a paste goes to the playhead inside the clip, and notes past the clip's end are left out", () => {
  const f = open([note(0)], BAR, BAR + 3 * BEAT);
  f.memory.copied = [note(0), note(2 * BEAT)];
  noteCommand(f.song, "paste");
  expect(f.notes().map((each) => each.start)).toEqual([0, 3 * BEAT]);
  expect(f.memory.notes).toEqual([`${3 * BEAT}:60:960:100`]);
});

test("notes pasted into a clip with no length are kept", () => {
  const f = open([note(0)], null);
  f.memory.copied = [note(0), note(8 * BEAT)];
  noteCommand(f.song, "paste");
  expect(f.notes().length).toBe(3);
});

test("a clip with no notes takes N, and N again makes a row up to its end", () => {
  const f = open([], BEAT);
  f.memory.length = BEAT / 2;
  expect(noteCommand(f.song, "add")).toBe(true);
  expect(noteCommand(f.song, "add")).toBe(true);
  expect(noteCommand(f.song, "add")).toBe(false);
  expect(f.notes().map((each) => each.start)).toEqual([0, BEAT / 2]);
});

test("a chosen note stays chosen when the notes move in the list, and one an undo took is not chosen", () => {
  const f = open([note(0), note(BEAT, 64)]);
  f.memory.notes = [`${BEAT}:64:960:100`];
  (f.self.children[0] as any).children[1].state.notes.reverse();
  noteCommand(f.song, "louder");
  expect(f.notes().find((each) => each.pitch === 64)?.velocity).toBe(110);
  (f.self.children[0] as any).children[1].state.notes = [note(0)];
  expect(noteCommand(f.song, "louder")).toBe(false);
});

test("alt-up steps the chosen velocities by 10, up to 127", () => {
  const f = open([note(0, 60, 120)]);
  noteCommand(f.song, "all");
  noteCommand(f.song, "louder");
  expect(f.notes()[0]!.velocity).toBe(127);
});

test("a velocity bar's drag changes every chosen note, and a line draws rising velocities", () => {
  const f = open([note(0), note(BEAT), note(2 * BEAT)]);
  noteCommand(f.song, "all");
  f.edits.velocity({ dy: 20 });
  expect(f.notes().map((each) => each.velocity)).toEqual([80, 80, 80]);
  f.edits.drawVelocities({ x: 0, y: 127 - 20, dx: 2 * BEAT, dy: -100 });
  expect(f.notes().map((each) => each.velocity)).toEqual([20, 70, 120]);
  expect(f.steps.at(-1)).toBe("Draw velocities");
});

test("the values of the head are typed, and unreadable text changes nothing", () => {
  const f = open([note(0), note(BEAT, 64)]);
  noteCommand(f.song, "all");
  f.edits.typePitch("D4");
  expect(f.notes().map((each) => each.pitch)).toEqual([62, 66]);
  f.edits.typeLength("3/16");
  expect(f.notes()[0]!.length).toBe(3 * (BEAT / 4));
  f.edits.typeVelocity("90");
  f.edits.typeVelocity("loud");
  f.edits.typePitch("H2");
  expect(f.notes().map((each) => each.velocity)).toEqual([90, 90]);
  f.edits.typeStart(BAR + 2 * BEAT);
  expect(f.notes().map((each) => each.start)).toEqual([2 * BEAT, 3 * BEAT]);
  expect(f.steps).toEqual(["Move notes", "Resize notes", "Change velocity", "Move notes"]);
});

test("pitches and lengths are said and read as the editor shows them", () => {
  expect([pitchName(60), pitchName(61), pitchOf("C#4"), pitchOf("Bb3"), pitchOf("c9"), pitchOf("H1")]).toEqual(["C4", "C#4", 61, 58, 120, null]);
  expect([lengthText(3 * 240, 240), lengthText(BEAT, 0), lengthText(120, 240)]).toEqual(["3/16", "4/16", "1/32"]);
});

test("quantize moves the chosen notes to the snap, or all while none is, and stays inside the clip", () => {
  const f = open([note(250), note(BAR - 10)]);
  f.memory.snap = BEAT;
  noteCommand(f.song, "quantize");
  expect(f.notes().map((each) => each.start)).toEqual([0, BAR - 1]);
  expect(f.steps).toEqual(["Quantize notes"]);
});

test("a point added in the bend lane lands on the grid at its height, and the last removed leaves it empty", () => {
  const f = open([]);
  f.edits.addPoint("bend", { x: 500, y: 0.25 });
  const clip = () => (f.self.children[0] as any).children[1].state;
  expect(clip().bend).toEqual([{ tick: 480, value: 4095 }]);
  f.edits.addPoint(7, { x: 0, y: 0 });
  expect(clip().controllers).toEqual({ 7: [{ tick: 0, value: 127 }] });
  f.edits.removePoint(7, 0);
  expect(clip().controllers).toEqual({});
});

test("a drag of the start keeps the end, down to 1/64, and shift-left and shift-right resize by the snap", () => {
  const f = open([note(BEAT)]);
  noteCommand(f.song, "all");
  f.edits.resizeStart({ dx: -BEAT / 2 });
  expect(f.notes()[0]).toEqual(note(BEAT / 2, 60, 100, 1.5 * BEAT));
  f.edits.resizeStart({ dx: 5 * BEAT });
  expect(f.notes()[0]).toEqual(note(2 * BEAT - BEAT / 16, 60, 100, BEAT / 16));
  noteCommand(f.song, "longer");
  noteCommand(f.song, "shorter");
  noteCommand(f.song, "shorter");
  expect(f.notes()[0]!.length).toBe(BEAT / 16);
  expect(f.steps).toEqual(["Resize notes", "Resize notes", "Resize notes", "Resize notes", "Resize notes"]);
});

test("notes moved past 0 or the clip's end stop there together, keeping their rhythm, and none goes", () => {
  const f = open([note(0), note(BEAT, 64), note(3 * BEAT, 67)]);
  noteCommand(f.song, "all");
  noteCommand(f.song, "left");
  f.edits.move({ dx: -BEAT, dy: 0, alt: false });
  expect(f.notes().map((each) => each.start)).toEqual([0, BEAT, 3 * BEAT]);
  f.edits.move({ dx: 2 * BAR, dy: 0, alt: false });
  expect(f.notes().map((each) => each.start)).toEqual([BAR - 1 - 3 * BEAT, BAR - 1 - 2 * BEAT, BAR - 1]);
  f.edits.move({ dx: 0, dy: -100, alt: false });
  expect(f.notes().map((each) => each.pitch)).toEqual([67, 71, 74].map((pitch) => pitch + 127 - 74));
  expect(f.memory.notes.length).toBe(3);
});

test("the starts of notes dragged past 0 stop there together", () => {
  const f = open([note(BEAT / 2), note(BEAT)]);
  noteCommand(f.song, "all");
  f.edits.resizeStart({ dx: -BEAT });
  expect(f.notes().map((each) => each.start)).toEqual([0, BEAT / 2]);
});

test("in a clip that starts off the grid, a double click, N and the arrow keys land on the song's grid", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("c00", 1000, BAR, [])])] }, { tick: 1300 });
  f.memory.below = { kind: "editor", clip: "t00/c00" };
  const notes = () => (f.self.children[0] as any).children[1].state.notes as ReturnType<typeof note>[];
  editsOf(f.song).notes("t00/c00").pressEmpty({ x: 300, y: 127 - 64, clicks: 2, shift: false, cmd: false });
  expect(notes().map((each) => 1000 + each.start)).toEqual([1200]);
  f.memory.notes = [];
  noteCommand(f.song, "add");
  expect(notes().map((each) => 1000 + each.start)).toEqual([1200, 1200]);
  f.memory.snap = BEAT;
  noteCommand(f.song, "right");
  expect(notes().map((each) => 1000 + each.start)).toEqual([1200, 2160]);
});
