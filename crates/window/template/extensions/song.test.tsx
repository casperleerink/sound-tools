import { expect, test } from "bun:test";
import { act, apply, find, h, press, render, type Self } from "./sdk";
import { song } from "./song";

type Song = Self<typeof song>;
const track = (name: string, title: string, mute: boolean) => ({
  name,
  tool: "track",
  state: { title, colour: "blue", automation: [] },
  children: [{ name: "mix", tool: "fader", state: { gain_db: 0, pan: 0, mute }, children: [] }],
});
const self = (mute: boolean) => ({ name: "song", state: { order: [], solo: [], sidechains: [], limit_off: false }, children: [track("t00", "Track 1", false), track("t01", "Track 2", mute)] }) as unknown as Song;

test("mute is lit only on a muted track", () => {
  render(song, self(true));
  expect(find("Mute Track 2")).toMatchObject({ checked: true });
  expect(find("Mute Track 1")).toMatchObject({ checked: false });
});

test("a press on mute mutes that track's fader, and a second unmutes it", () => {
  const first = self(false);
  render(song, first);
  const changes = press("mute-t01");
  expect(changes).toEqual([[["children", 1, "children", 0, "state", "mute"], true]]);
  render(song, apply(first, changes));
  expect(find("mute-t01")).toMatchObject({ checked: true });
  expect(press("mute-t01")).toEqual([[["children", 1, "children", 0, "state", "mute"], false]]);
});

test("a fader takes a volume by its label", () => {
  render(song, self(false));
  expect(act("Volume Track 1", "set", -6)).toEqual([[["children", 0, "children", 0, "state", "gain_db"], -6]]);
});

test("choosing a track shows its devices", () => {
  render(song, self(false));
  press("track-t01");
  expect(find("track-t01")).toMatchObject({ checked: true });
});

test("a press mutes the track it names, also after tracks come and go", () => {
  const tracks = (...names: string[]) => ({ ...self(false), children: names.map((name) => track(name, `Track ${Number(name.slice(1)) + 1}`, false)) }) as unknown as Song;
  render(song, tracks("t00", "t01", "t02"));
  render(song, tracks("t00", "t02", "t03"));
  expect(act("Mute Track 4", "toggle")).toEqual([[["children", 2, "children", 0, "state", "mute"], true]]);
});

test("a fader a lane moves takes no change", () => {
  render(song, self(false), { held: ["t00/mix.gain_db"] });
  expect(() => act("Volume Track 1", "set", -6)).toThrow();
  expect(act("Volume Track 2", "set", -6)).toEqual([[["children", 1, "children", 0, "state", "gain_db"], -6]]);
});

test("audio tracks are rows and returns fold under them: a return has no solo, and only an audio track arms", () => {
  const fader = { name: "mix", tool: "fader", state: { gain_db: 0, pan: 0, mute: false }, children: [] };
  const audio = { name: "audio-1", tool: "tape", state: { title: "", colour: "teal", automation: [] }, children: [fader] };
  const back = { name: "ret-a", tool: "bus", state: { title: "", colour: "teal", pre_fader: false }, children: [fader] };
  render(song, { ...self(false), children: [...self(false).children, audio, back] } as unknown as Song);
  // The returns fold into one row until it opens them.
  expect(find("Choose Return A")).toBeUndefined();
  press("returns-header");
  expect([find("Choose Audio 1"), find("Choose Return A"), find("Arm Audio 1")]).toMatchObject([{ role: "tab" }, { role: "tab" }, { role: "switch" }]);
  expect([find("Solo Return A"), find("Arm Track 1")]).toEqual([undefined, undefined]);
});

test("the song opens on its first track's panel; escape closes it and the master shows its own", () => {
  // Another view first: the song's memory starts as it does when the song opens.
  const other = (_self: { name: string; children: never[] }) => <box />;
  render(other, { name: "other", children: [] });
  render(song, self(false));
  expect(find("strip-t00")).toBeDefined();
  act("song.back");
  expect(find("strip-t00")).toBeUndefined();
  press("master");
  expect(find("strip-master")).toBeDefined();
  act("Close the panel", "press");
  expect(find("strip-master")).toBeUndefined();
});
