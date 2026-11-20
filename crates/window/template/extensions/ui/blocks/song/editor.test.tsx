import { expect, test } from "bun:test";
import { act, find, h, press, render, type Node, type Self } from "../../../sdk";
import type { song } from "../../../song";
import { clipOf, trackOf } from "../../../song/fake";
import { memory as fresh, type Memory, type Song } from "../../../song/model";
import { BAR, BEAT } from "../../../song/time";
import { pageOf } from "../../../song/view";
import { Editor } from "./editor";

type Drawn = Self<typeof song>;
type Shown = { start: number; length: number; pitch: number; velocity: number };
const note = (start: number, pitch = 60, velocity = 100, length = BEAT): Shown => ({ start, length, pitch, velocity });
const ROLL = "roll-t00-c00";

/** The editor of the clip `t00/c00` alone, as the song draws it, with the song's memory. */
function shown(notes: Shown[], { length = BAR as number | null, track = trackOf("t00", [clipOf("c00", BAR, length, notes)]) } = {}) {
  let memory = undefined as unknown as Memory;
  const draw = (self: Drawn, ctx: Song) => {
    if (!("rolls" in ctx.memory)) Object.assign(ctx.memory, fresh(), { below: { kind: "editor", clip: "t00/c00" } });
    memory = ctx.memory;
    return <Editor page={pageOf(self, ctx)} clip="t00/c00" />;
  };
  let self = { name: "song", state: { order: [], solo: [], sidechains: [], limit_off: false }, children: [track] } as unknown as Drawn;
  render(draw, self);
  return {
    get memory() { return memory; },
    /** The notes of the clip after the changes `ops` asked for, drawn again as the window draws them. */
    notes: () => ((self.children[0] as any).children[1].state.notes as Shown[]),
    apply(ops: ReturnType<typeof act>) {
      for (const [path, value] of ops) {
        let at: any = self;
        path.slice(0, -1).forEach((step) => (at = at[step]));
        at[path.at(-1)!] = value;
      }
      self = structuredClone(self);
      render(draw, self);
    },
    /** The row of the roll a pitch is on. */
    row: (pitch: number) => 127 - pitch - memory.rolls["t00/c00"]!.top,
  };
}

/** The words a part of the view shows. */
const words = (node: Node | undefined): string[] =>
  !node ? [] : node.t === "text" ? [node.text] : "children" in node && Array.isArray(node.children) ? node.children.flatMap((child) => words(child as Node)) : [];

test("the head says the chosen note only while one is chosen: its pitch, its start in the song, its length and velocity", () => {
  const editor = shown([note(BEAT / 4, 61, 90, 3 * (BEAT / 4)), note(BEAT, 64)]);
  expect(find("note-pitch")).toBeUndefined();
  editor.apply(press("note-0"));
  expect([find("note-pitch"), find("note-start"), find("note-length"), find("note-velocity")]).toMatchObject([{ value: "C#4" }, { value: "2.1.2" }, { value: "3/16" }, { value: "90" }]);
  editor.apply(press("note-1", { shift: true }));
  expect(editor.memory.notes.length).toBe(2);
  expect(find("note-pitch")).toMatchObject({ value: "C#4" });
  editor.apply(press(ROLL, { x: 3000, y: 0 }));
  expect(find("note-pitch")).toBeUndefined();
});

test("a value of the head is typed: a start as bar.beat.sixteenth moves the chosen notes, and text it cannot read changes nothing", () => {
  const editor = shown([note(0), note(BEAT, 64)]);
  editor.apply(press("note-0"));
  editor.apply(press("note-1", { cmd: true }));
  editor.apply(act("note-start-typed", "commit", "2.2.3"));
  expect(editor.notes().map((each) => each.start)).toEqual([BEAT + BEAT / 2, 2 * BEAT + BEAT / 2]);
  expect(act("note-start-typed", "commit", "later")).toEqual([]);
  expect(act("note-pitch-typed", "commit", "C4")).toEqual([]);
});

test("a long clip opens fitted, so a double click near its end adds a note there, on the snap, and a double click on it takes it out", () => {
  const editor = shown([note(0)], { length: 64 * BAR });
  const { across } = editor.memory.rolls["t00/c00"]!;
  expect(64 * BAR * across).toBeLessThan(1100);
  editor.apply(act(ROLL, "press", { x: 64 * BAR - BAR / 2 + 100, y: editor.row(62) + 0.5, clicks: 2 }));
  expect(editor.notes()[1]).toEqual(note(64 * BAR - BAR / 2, 62, 100, BEAT / 2));
  editor.apply(act("note-1", "press", { clicks: 2 }));
  expect(editor.notes()).toEqual([note(0)]);
});

test("the roll opens with the middle of the notes in its middle", () => {
  const editor = shown([note(0, 84), note(BEAT, 96)]);
  const rows = (editor.memory.editor - 32 - 64) / 12;
  expect(Math.abs(editor.row(90) - rows / 2)).toBeLessThan(1);
});

test("a drag of a chosen note moves every chosen one on the snap, up a row a pitch, as one change", () => {
  const editor = shown([note(0), note(BEAT, 64), note(2 * BEAT, 67)]);
  editor.apply(press("note-0"));
  editor.apply(press("note-2", { shift: true }));
  const ops = act("note-0", "drag", { dx: 250, dy: -2 });
  editor.apply(ops);
  expect(editor.notes()).toEqual([note(240, 62), note(BEAT, 64), note(2 * BEAT + 240, 69)]);
  expect(ops.length).toBeGreaterThan(0);
  expect(find("note-0")).toMatchObject({ value: true });
  expect(find("note-1")).toMatchObject({ value: false });
});

test("a drag of a note not chosen moves it alone, and with alt copies it", () => {
  const editor = shown([note(0), note(BEAT, 64)]);
  editor.apply(press("note-0"));
  editor.apply(act("note-1", "drag", { dx: BEAT, dy: 0, alt: true }));
  expect(editor.notes().map((each) => each.start)).toEqual([0, BEAT, 2 * BEAT]);
  expect(find("note-2")).toMatchObject({ value: true });
  expect(find("note-0")).toMatchObject({ value: false });
});

test("a chosen note's ends resize it on the snap, the start keeping its end; one not chosen has none", () => {
  const editor = shown([note(0)]);
  expect(find("note-0-end")).toBeUndefined();
  editor.apply(press("note-0"));
  editor.apply(act("note-0-end", "drag", { dx: 130 }));
  expect(editor.notes()[0]!.length).toBe(BEAT + 240);
  editor.apply(act("note-0-start", "drag", { dx: 470 }));
  expect(editor.notes()[0]).toMatchObject({ start: 480, length: BEAT + 240 - 480 });
});

test("a drag over empty roll chooses the notes it touches, and shift adds to them", () => {
  const editor = shown([note(0, 60), note(BEAT, 72), note(2 * BEAT, 60)]);
  editor.apply(act(ROLL, "drag", { x: 100, y: editor.row(60) + 0.2, dx: BEAT, dy: 0.5 }));
  expect([find("note-0"), find("note-1"), find("note-2")]).toMatchObject([{ value: true }, { value: false }, { value: false }]);
  editor.apply(act(ROLL, "drag", { x: 2 * BEAT + 10, y: editor.row(60) + 0.5, dx: 10, dy: 0.2, shift: true }));
  expect(editor.memory.notes.length).toBe(2);
});

test("a right press on a note chooses it and opens the note menu; on empty roll with none chosen the menu adds and pastes", () => {
  const editor = shown([note(0), note(BEAT, 64)]);
  press(ROLL, { x: 3000, y: editor.row(50), right: true });
  expect([find("Add a note"), find("Cut")]).toMatchObject([{ role: "menuitem" }, undefined]);
  press("note-1", { right: true });
  expect(find("note-1")).toMatchObject({ value: true });
  editor.apply(act("Duplicate", "press"));
  expect(editor.notes().map((each) => each.start)).toEqual([0, BEAT, 2 * BEAT]);
  expect(find("Strength 100%")).toMatchObject({ value: true });
});

test("a drag of a velocity bar changes every chosen note, and a line across the lane draws velocities", () => {
  const editor = shown([note(0), note(BEAT), note(2 * BEAT)]);
  editor.apply(press("note-0"));
  editor.apply(press("note-1", { shift: true }));
  editor.apply(act("velocity-1", "drag", { dy: 30 }));
  expect(editor.notes().map((each) => each.velocity)).toEqual([70, 70, 100]);
  editor.apply(act(`${ROLL}-velocity`, "drag", { x: 0, y: 127 - 20, dx: 2 * BEAT, dy: -100 }));
  expect(editor.notes().map((each) => each.velocity)).toEqual([20, 70, 120]);
});

test("the lane picker offers the clip's own controllers by name, and every controller where a plugin plays the track", () => {
  const clip = { ...clipOf("c00", BAR, BAR), state: { ...clipOf("c00", BAR, BAR).state, controllers: { 74: [{ tick: 0, value: 64 }] } } };
  shown([], { track: trackOf("t00", [clip]) });
  expect(find("editor-lane-74")).toMatchObject({ label: "Brightness" });
  expect(words(find("editor-lane-74"))).toEqual(["Brightness", "Controller 74"]);
  expect(find("editor-lane-7")).toBeUndefined();
  shown([], { track: trackOf("t00", [{ name: "synth", tool: "plugin", state: {}, children: [] }, clipOf("c00", BAR, BAR)]) });
  expect(find("editor-lane-7")).toMatchObject({ label: "Volume" });
});

test("in the bend lane a double click adds a point on the snap, a drag moves it, and alt and a press takes it out", () => {
  const editor = shown([]);
  editor.apply(act("editor-lane-bend", "press"));
  expect(find(`${ROLL}-points`)).toMatchObject({ label: "Bend lane" });
  editor.apply(act(`${ROLL}-points`, "press", { x: 500, y: 0.25, clicks: 2 }));
  expect(find("point-0")).toMatchObject({ value: 4095 });
  editor.apply(act("point-0", "drag", { dx: 250, dy: 0.25 }));
  expect(find("point-0")).toMatchObject({ value: -1, drag: { from: [720, 0.5000305194408838] } });
  editor.apply(act("point-0", "press", { alt: true }));
  expect(find("point-0")).toBeUndefined();
});

test("an empty clip says how to add a note", () => {
  shown([]);
  expect(words(find("notes-empty"))).toEqual(["No notes yet", "Double-click to add a note, or press N."]);
  shown([note(0)]);
  expect(find("notes-empty")).toBeUndefined();
});

test("the name types a new title; the size button says its key; close shows the track's panel", () => {
  const editor = shown([note(0)]);
  editor.apply(act("clip-name-typed", "commit", "Verse"));
  expect(find("clip-name")).toMatchObject({ value: "Verse" });
  expect(words(find("editor-size"))).toContain("Larger");
  act("editor-size", "press");
  expect(editor.memory.editorLarge).toBe(true);
  act("close-editor", "press");
  expect(editor.memory.below).toEqual({ kind: "track", name: "t00" });
});

test("a key of the piano says what it plays, and a note is a button the keys reach", () => {
  shown([note(0)]);
  expect(find("Play C4")).toMatchObject({ role: "button" });
  expect(find("note-0")).toMatchObject({ role: "button", label: "Note C4 at 2.1.1" });
});

test("an edit of one note draws only its bar again: of 2,000 notes, the other bars' memos are kept", () => {
  const many = Array.from({ length: 2000 }, (_, at) => note(at * (BEAT / 4), 36 + ((at * 7) % 48), 100, BEAT / 4));
  const editor = shown(many, { length: null });
  const [far, near] = [find("note-1500"), find("note-5")];
  editor.apply(act("note-5", "drag", { dx: 0, dy: -1 }));
  expect(find("note-1500")).toBe(far);
  expect(find("note-5")).not.toBe(near);
});
