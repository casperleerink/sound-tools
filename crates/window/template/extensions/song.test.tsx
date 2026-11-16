import { expect, test } from "bun:test";
import { act, apply, find, press, render, type Self } from "./sdk/view";
import { song, songView } from "./song";

type Song = Self<typeof song>;
const track = (name: string, title: string, mute: boolean) => ({
  name,
  tool: "track",
  state: { title, colour: "blue", automation: [] },
  children: [{ name: "mix", tool: "fader", state: { gain_db: 0, pan: 0, mute }, children: [] }],
});
const self = (mute: boolean) => ({ name: "song", state: { order: [], solo: [], sidechains: [], limit_off: false }, children: [track("t00", "Track 1", false), track("t01", "Track 2", mute)] }) as unknown as Song;

test("mute is lit only on a muted track", () => {
  render(songView, self(true));
  expect(find("Mute Track 2")).toMatchObject({ checked: true });
  expect(find("Mute Track 1")).toMatchObject({ checked: false });
});

test("a press on mute mutes that track's fader, and a second unmutes it", () => {
  const first = self(false);
  render(songView, first);
  const changes = press("mute-t01");
  expect(changes).toEqual([[["children", 1, "children", 0, "state", "mute"], true]]);
  render(songView, apply(first, changes));
  expect(find("mute-t01")).toMatchObject({ checked: true });
  expect(press("mute-t01")).toEqual([[["children", 1, "children", 0, "state", "mute"], false]]);
});

test("a fader takes a volume by its label", () => {
  render(songView, self(false));
  expect(act("Volume Track 1", "set", -6)).toEqual([[["children", 0, "children", 0, "state", "gain_db"], -6]]);
});

test("choosing a track shows its devices", () => {
  render(songView, self(false));
  press("track-t01");
  expect(find("track-t01")).toMatchObject({ checked: true });
});

test("a press mutes the track it names, also after tracks come and go", () => {
  const tracks = (...names: string[]) => ({ ...self(false), children: names.map((name) => track(name, `Track ${Number(name.slice(1)) + 1}`, false)) }) as unknown as Song;
  render(songView, tracks("t00", "t01", "t02"));
  render(songView, tracks("t00", "t02", "t03"));
  expect(act("Mute Track 4", "toggle")).toEqual([[["children", 2, "children", 0, "state", "mute"], true]]);
});
