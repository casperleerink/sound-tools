import { expect, test } from "bun:test";
import { find, press, render } from "./sdk";
import { view, type State } from "./song";

const track = (solo: boolean) => ({ name: "Bass", colour: "#7ee0a0" as const, clips: [], mute: false, solo, armed: false, gain_db: 0 });
const state = (solo: boolean): State => ({ song: { tracks: [track(false), track(solo)] }, playing: false, view: { zoom: 0.025, page: 0 } });

test("solo is lit only on a soloed track", () => {
  render(view, state(true));
  expect(find("solo-1")).toMatchObject({ fill: "green" });
  render(view, state(false));
  expect(find("solo-1")).toMatchObject({ fill: "border" });
});

test("a press on solo asks to solo that track and no other", () => {
  render(view, state(false));
  expect(press("solo-1")).toEqual([[["song", "tracks", 1, "solo"], true]]);
});
