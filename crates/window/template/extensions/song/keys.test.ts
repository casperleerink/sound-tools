import { expect, test } from "bun:test";
import type { ActionKey } from "../sdk";
import { clipOf, fakeSong, laneOf, trackOf } from "./fake";
import { actions } from "./keys";
import type { Song } from "./model";
import { BAR, BEAT } from "./time";

/** A key as the window gives it: the song's actions with that key, from the top, until one does it. */
const press = (song: Song, key: ActionKey) => {
  for (const [name, action] of Object.entries(actions)) {
    if (action.keys?.includes(key) && action.run(song) !== false) return name;
  }
  return null;
};
const note = (start: number) => ({ start, length: BEAT, pitch: 60, velocity: 100 });
const song = () =>
  fakeSong(
    { children: [trackOf("t00", [clipOf("c00", 0, BAR, [note(0), note(BEAT)]), laneOf("lane-pan", [{ tick: 0, value: 0 }, { tick: BEAT, value: 1 }])], [{ lane: "lane-pan", target: "mix.pan" }])] },
    { piece: { sections: [{ bar: 2, name: "Verse" }] } },
  );

test("delete takes the chosen notes first, then the chosen mark, point and clips, and last asks for the track", () => {
  const f = song();
  f.memory.below = { kind: "editor", clip: "t00/c00" };
  f.memory.selected = ["t00/c00"];
  f.memory.notes = ["0:60:960:100"];
  f.memory.mark = { kind: "section", bar: 2, tick: BAR };
  f.memory.point = { track: "t00", lane: "lane-pan", tick: BEAT };
  expect(press(f.song, "delete")).toBe("delete_notes");
  expect(press(f.song, "delete")).toBe("delete_mark");
  expect(press(f.song, "delete")).toBe("delete_point");
  expect(press(f.song, "delete")).toBe("delete_clips");
  f.memory.below = { kind: "track", name: "t00" };
  expect(press(f.song, "delete")).toBe("delete_track");
  expect(f.steps).toEqual(["Delete notes", "Remove a section", "Remove a point", "Remove clips"]);
});

test("escape lets go one thing at a time, then closes the editor and what shows below", () => {
  const f = song();
  f.memory.below = { kind: "editor", clip: "t00/c00" };
  f.memory.selected = ["t00/c00"];
  f.memory.notes = ["0:60:960:100"];
  press(f.song, "escape");
  expect(f.memory.notes).toEqual([]);
  press(f.song, "escape");
  expect(f.memory.below).toEqual({ kind: "track", name: "t00" });
  press(f.song, "escape");
  expect(f.memory.selected).toEqual([]);
  press(f.song, "escape");
  expect(f.memory.below).toBe(null);
});

test("cmd-a chooses all notes while the editor is open, else all clips; enter opens the chosen clip's notes", () => {
  const f = song();
  expect(press(f.song, "cmd-a")).toBe("select_all_clips");
  expect(press(f.song, "enter")).toBe("open_clip");
  expect(f.memory.below).toEqual({ kind: "editor", clip: "t00/c00" });
  expect(press(f.song, "cmd-a")).toBe("select_all_notes");
  expect(f.memory.notes.length).toBe(2);
});

test("enter names the chosen section, and alt-enter asks for the menu of the chosen clip", () => {
  const f = song();
  press(f.song, "alt-shift-right");
  expect(press(f.song, "enter")).toBe("name_section");
  expect(f.memory.renamingSection).toEqual({ bar: 2, tick: BAR });
  f.memory.selected = ["t00/c00"];
  press(f.song, "alt-enter");
  expect(f.memory.menu).toBe("t00/c00");
});

test("the keys add a clip at the playhead and notes in a row in it", () => {
  const f = fakeSong({ children: [trackOf("t00")] }, { tick: 2 * BAR });
  f.memory.track = "t00";
  press(f.song, "shift-c");
  press(f.song, "enter");
  press(f.song, "n");
  press(f.song, "n");
  const clip = (f.self.children[0] as any).children[1].state;
  expect(clip).toMatchObject({ start: 2 * BAR, length: BAR });
  expect(clip.notes.map((each: { start: number }) => each.start)).toEqual([0, BEAT / 2]);
});

test("shift-enter makes the open editor large and puts it back, with no undo step", () => {
  const f = song();
  expect(press(f.song, "shift-enter")).toBe(null);
  f.memory.below = { kind: "editor", clip: "t00/c00" };
  press(f.song, "shift-enter");
  expect(f.memory.editorLarge).toBe(true);
  expect(f.steps).toEqual([]);
});
