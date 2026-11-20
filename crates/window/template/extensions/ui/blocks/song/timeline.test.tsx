import { expect, test } from "bun:test";
import { act, apply, find, h, press, render, type Op, type Self } from "../../../sdk";
import { song } from "../../../song";
import { clipOf, regionOf, tapeOf, trackOf } from "../../../song/fake";
import { BAR, BEAT } from "../../../song/time";

type Song = Self<typeof song>;
const notes = [{ start: 0, length: 480, pitch: 60, velocity: 100 }];
const songOf = (children: unknown[], order: string[] = []) => ({ name: "song", state: { order, solo: [], sidechains: [], limit_off: false }, children }) as unknown as Song;
/** Two tracks, a clip each, and an audio track with a region. */
const two = () => songOf([trackOf("t00", [clipOf("a", 0, BAR, notes)]), trackOf("t01", [clipOf("b", BAR, BAR, notes)]), tapeOf("audio-1", [regionOf("r", 0, 2)])], ["t00", "t01", "audio-1"]);

/** Renders the song as it opens: its memory new, as another view ran before. */
function opened(self: Song) {
  const other = (_self: { name: string; children: never[] }) => <box />;
  render(other, { name: "other", children: [] });
  return render(song, self);
}
/** The value the changes `ops` set at the end of `path`, of the last that does. */
const setAt = (ops: Op[], ...path: (string | number)[]) => ops.findLast(([at]) => path.every((step, index) => at.at(index - path.length) === step))?.[1];
test("a clip moves on the snap: sixteenths, then bars, then to the tick with snap off, and off the grid with cmd", () => {
  opened(two());
  expect(setAt(act("clip-t00/a", "drag", { dx: 1000 }), "start")).toBe(960);
  expect(setAt(act("clip-t00/a", "drag", { dx: 1000, cmd: true }), "start")).toBe(1960);
  press(`snap-${BAR}`);
  expect(setAt(act("clip-t00/a", "drag", { dx: 100 }), "start")).toBe(BAR);
  press("snap-0");
  expect(setAt(act("clip-t00/a", "drag", { dx: 1001 }), "start")).toBe(BAR + 1001);
});

test("alt copies a clip onto the track under it and leaves the original", () => {
  const self = two();
  opened(self);
  const ops = act("clip-t00/a", "drag", { dx: 0, dy: 1, alt: true });
  const after = apply(self, ops);
  expect(after.children[0]!.children.map((child) => child.name)).toEqual(["mix", "a"]);
  expect(after.children[1]!.children.filter((child) => child.tool === "timeline").map((child) => child.state)).toMatchObject([{ start: BAR }, { start: 0 }]);
});

test("a clip dragged onto an audio track stays on its own track", () => {
  const self = two();
  opened(self);
  const after = apply(self, act("clip-t01/b", "drag", { dx: 960, dy: 1 }));
  expect([after.children[1]!.children.at(-1), after.children[2]!.children.some((child) => child.tool === "timeline")]).toMatchObject([{ name: "b", state: { start: BAR + 960 } }, false]);
});

test("a drag over empty space chooses the clips and regions it covers, shift adds", () => {
  opened(two());
  act("song", "drag", { x: BEAT, y: 0.5, dx: BAR, dy: 1 });
  expect([find("clip-t00/a"), find("clip-t01/b"), find("clip-audio-1/r")]).toMatchObject([{ checked: true }, { checked: true }, { checked: false }]);
  act("song", "drag", { x: 0, y: 2.5, dx: 100, dy: 0, shift: true });
  expect([find("clip-t00/a"), find("clip-audio-1/r")]).toMatchObject([{ checked: true }, { checked: true }]);
  press("song");
  expect(find("clip-t00/a")).toMatchObject({ checked: false });
});

test("a double click on empty space of a track adds a bar long clip in the cell under it, chosen; on an audio track none", () => {
  const self = two();
  opened(self);
  const ops = act("song", "press", { clicks: 2, x: 5000, y: 1.5 });
  expect(apply(self, ops).children[1]!.children.at(-1)).toMatchObject({ tool: "timeline", state: { start: 4800, length: BAR } });
  render(song, apply(self, ops));
  expect(find("clip-t01/clip")).toMatchObject({ checked: true });
  expect(act("song", "press", { clicks: 2, x: 5000, y: 2.5 })).toEqual([]);
});

test("the timeline reaches 32 bars past its last clip, and 64 bars at least", () => {
  opened(songOf([trackOf("t00", [clipOf("a", 100 * BAR, BAR)])]));
  const extent = find("song") as { children: { place?: number[] }[] };
  expect(extent.children[0]!.place).toEqual([0, 0, 133 * BAR, 2]);
  opened(two());
  expect((find("song") as { children: { place?: number[] }[] }).children[0]!.place![2]).toBe(96 * BAR);
});

test("a double click on a clip opens its notes below; escape puts its track's panel back", () => {
  opened(two());
  act("clip-t01/b", "press", { clicks: 2 });
  expect(find("close-editor")).toBeDefined();
  act("song.back");
  expect([find("close-editor"), find("strip-t01")]).toMatchObject([undefined, { key: "strip-t01" }]);
});

test("a clip's menu opens on a right press, shows the keys, and renames the clip in place", () => {
  const self = two();
  opened(self);
  expect(find("menu-t00/a-Cut")).toBeUndefined();
  press("clip-t00/a", { right: true });
  expect(find("Cut")).toMatchObject({ key: "menu-t00/a-Cut" });
  expect(JSON.stringify(find("menu-t00/a-Copy"))).toContain("⌘C");
  press("menu-t00/a-Rename");
  const ops = act("clip-name-t00/a", "commit", "Verse");
  expect(setAt(ops, "title")).toBe("Verse");
  render(song, apply(self, ops));
  expect(find("Clip Verse")).toMatchObject({ key: "clip-t00/a" });
});

test("a region has no rename in its menu", () => {
  opened(two());
  press("clip-audio-1/r", { right: true });
  expect([find("menu-audio-1/r-Cut"), find("menu-audio-1/r-Rename")]).toMatchObject([{ label: "Cut" }, undefined]);
});

test("the end of a chosen clip resizes it on the snap, its start keeps its notes where they are", () => {
  opened(two());
  press("clip-t01/b");
  expect(setAt(act("clip-end-t01/b", "drag", { dx: -1000 }), "length")).toBe(BAR - 960);
  expect(setAt(act("clip-start-t01/b", "drag", { dx: -1000 }), "state", "start")).toBe(BAR - 960);
});

test("an edit of one clip draws its row again and no other", () => {
  const self = two();
  opened(self);
  const [a, b] = [find("clip-t00/a"), find("clip-t01/b")];
  render(song, apply(self, act("clip-t00/a", "drag", { dx: 960 })));
  expect(find("clip-t01/b")).toBe(b);
  expect(find("clip-t00/a")).not.toBe(a);
});

test("an empty song offers an instrument track and an audio track", () => {
  opened(songOf([]));
  expect([find("Add an instrument track"), find("Add an audio track"), find("add-track")]).toMatchObject([{ role: "button" }, { role: "button" }, undefined]);
});
