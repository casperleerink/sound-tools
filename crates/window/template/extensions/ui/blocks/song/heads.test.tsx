import { expect, test } from "bun:test";
import { act, apply, find, h, press, render, type Self } from "../../../sdk";
import { song } from "../../../song";
import { clipOf, tapeOf, trackOf } from "../../../song/fake";

type Song = Self<typeof song>;
const songOf = (children: unknown[], order: string[]) => ({ name: "song", state: { order, solo: [], sidechains: [], limit_off: false }, children }) as unknown as Song;
const three = () => songOf([trackOf("t00"), trackOf("t01"), tapeOf("audio-1")], ["t00", "t01", "audio-1"]);

/** Renders the song as it opens: its memory new, as another view ran before. */
function opened(self: Song) {
  const other = (_self: { name: string; children: never[] }) => <box />;
  render(other, { name: "other", children: [] });
  return render(song, self);
}

test("a double click on a head names its track in place: one step, and escape keeps the name", () => {
  const self = three();
  opened(self);
  act("Choose Track 2", "press", { clicks: 2 });
  expect(find("rename-t01")).toMatchObject({ label: "Name of Track 2", value: "Track 2" });
  expect(act("rename-t01", "commit", null)).toEqual([]);
  expect(find("rename-t01")).toBeUndefined();
  act("Choose Track 2", "press", { clicks: 2 });
  const ops = act("rename-t01", "commit", " Bass ");
  expect(ops).toEqual([[["children", 1, "state", "title"], "Bass"]]);
  render(song, apply(self, ops));
  expect(find("Choose Bass")).toMatchObject({ key: "track-t01" });
});

test("enter names the track whose panel shows", () => {
  opened(three());
  press("track-t01");
  act("song.rename_track");
  expect(find("rename-t01")).toBeDefined();
});

test("a track's mark picks one of the fourteen colours, its own ticked", () => {
  opened(three());
  const colours = find("Colour of Track 1") as { children: { children?: unknown[] }[] };
  const list = colours.children.find((child) => "role" in child && child.role === "menu");
  expect(list?.children?.length).toBe(14);
  expect(find("colour-t00-Blue")).toMatchObject({ checked: true });
  expect(press("colour-t00-Mauve")).toEqual([[["children", 0, "state", "colour"], "mauve"]]);
});

test("a head dragged onto another goes before it", () => {
  const self = three();
  opened(self);
  expect(apply(self, act("track-audio-1", "drag", { dy: -2, over: "track-t00" })).state.order).toEqual(["audio-1", "t00", "t01"]);
  expect(act("track-t00", "drag", { dy: 1, over: "mute-t01" })).toEqual([]);
});

test("a right press opens a track's menu with the key of each; delete asks first", async () => {
  opened(three());
  expect(find("menu-t00-Delete")).toBeUndefined();
  press("track-t00", { right: true });
  expect(JSON.stringify(find("menu-t00-Delete"))).toContain("⌫");
  expect(find("menu-t00-Render to audio track")).toMatchObject({ label: "Render to audio track" });
  // Cancel: the track stays.
  expect(press("menu-t00-Delete")).toEqual([]);
});

test("render says under it what rings out after the end, or that it stops at the end of the clips", () => {
  opened(songOf([trackOf("t00", [clipOf("c00", 0, 3840)])], ["t00"]));
  press("track-t00", { right: true });
  expect(JSON.stringify(find("menu-t00-Render to audio track"))).toContain("Reverbs and releases ring out after the end");
  expect(JSON.stringify(find("menu-t00-Render without the tail"))).toContain("It stops where the clips end");
});

test("a track with no clips cannot render, and says why", () => {
  opened(three());
  press("track-t00", { right: true });
  expect(JSON.stringify(find("menu-t00-Render to audio track"))).toContain("The track has no clips");
});

test("the chevron of Add track offers an audio track and a return; a head says what plays it", () => {
  const self = three();
  opened(self);
  expect([find("add-track-kind-Audio track"), find("add-track-kind-Return track")]).toMatchObject([{ role: "menuitem" }, { role: "menuitem" }]);
  expect(JSON.stringify(find("track-audio-1"))).toContain('"text":"Audio"');
});

test("mute, solo and arm show while on or chosen; only an audio track arms", () => {
  opened(three());
  expect(JSON.stringify(find("track-t01"))).toContain('"reveal":true');
  expect([find("Arm Audio 1"), find("Arm Track 1")]).toMatchObject([{ role: "switch" }, undefined]);
});

test("the returns and the master stay under the heads, out of the plane that scrolls", () => {
  const back = { name: "ret-a", tool: "bus", state: { title: "", colour: "teal", pre_fader: false }, children: [{ name: "mix", tool: "fader", state: { gain_db: 0, pan: 0, mute: false }, children: [] }] };
  opened(songOf([trackOf("t00"), back], ["t00"]));
  const heads = find("heads");
  expect([find("track-t00", heads), find("master", heads), find("returns-header", heads)]).toMatchObject([{ key: "track-t00" }, undefined, undefined]);
  expect([find("master"), find("returns-header")]).toMatchObject([{ role: "tab" }, { label: "Show the returns" }]);
});

test("the master's toggle shows the mixer in place of the panel, and hides it", () => {
  opened(three());
  press("mixer-toggle");
  expect([find("strip-master"), find("close-panel"), find("Hide the mixer")]).toMatchObject([{ key: "strip-master" }, undefined, { checked: true }]);
  press("mixer-toggle");
  expect(find("strip-master")).toBeUndefined();
});
