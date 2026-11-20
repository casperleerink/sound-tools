import { expect, test } from "bun:test";
import { editsOf } from "./edits";
import { fakeSong, laneOf, tapeOf, trackOf } from "./fake";
import { laneAt } from "./lanes";
import { BAR, BEAT } from "./time";

const child = (f: ReturnType<typeof fakeSong>, track: string, name: string) => (f.self.children as any[]).find((each) => each.name === track).children.find((each: { name: string }) => each.name === name);
const automation = (f: ReturnType<typeof fakeSong>, track: string) => (f.self.children as any[]).find((each) => each.name === track).state.automation;
const panned = () => fakeSong({ children: [trackOf("t00", [laneOf("lane-pan", [{ tick: 0, value: -1 }, { tick: BAR, value: 1 }])], [{ lane: "lane-pan", target: "mix.pan" }])] });

test("a lane added for a number starts where the number is, and the lanes show", () => {
  const f = fakeSong({ children: [tapeOf("audio-1")] });
  editsOf(f.song).lanes.add("audio-1", "mix.gain_db", -6);
  expect(child(f, "audio-1", "lane-mix-gain-db").state).toEqual({ points: [{ tick: 0, value: -6 }], scale: "fader" });
  expect(automation(f, "audio-1")).toEqual([{ lane: "lane-mix-gain-db", target: "mix.gain_db" }]);
  expect(f.memory.lanes).toEqual(["audio-1"]);
  expect(f.steps).toEqual(["Add a lane"]);
});

test("a press adds a point on the snap and its drag moves it, both one step; alt-click removes; the last takes the lane", () => {
  const f = panned();
  const { lanes } = editsOf(f.song);
  const tick = lanes.addPoint("t00", "lane-pan", { x: 500, y: 0.5, cmd: false });
  expect(tick).toBe(480);
  lanes.movePoint("t00", "lane-pan", tick, { dx: 240, dy: 0.25 }, true);
  expect(child(f, "t00", "lane-pan").state.points[1]).toEqual({ tick: 720, value: -0.5 });
  expect(f.memory.point).toEqual({ track: "t00", lane: "lane-pan", tick: 720 });
  expect(f.steps).toEqual(["Add a point", "Add a point"]);
  // A point does not pass its neighbours.
  lanes.movePoint("t00", "lane-pan", 720, { dx: 10 * BAR, dy: 0 });
  expect(child(f, "t00", "lane-pan").state.points[1].tick).toBe(BAR - 1);
  for (const point of [...child(f, "t00", "lane-pan").state.points]) lanes.removePoint("t00", "lane-pan", point.tick);
  expect(child(f, "t00", "lane-pan")).toBeUndefined();
  expect(automation(f, "t00")).toEqual([]);
});

test("an alt-drag erases the points it covers", () => {
  const f = fakeSong({ children: [trackOf("t00", [laneOf("lane-pan", [0, 1, 2, 3].map((beat) => ({ tick: beat * BEAT, value: 0 })))], [{ lane: "lane-pan", target: "mix.pan" }])] });
  editsOf(f.song).lanes.erase("t00", "lane-pan", { x: 2.5 * BEAT, dx: -2 * BEAT });
  expect(child(f, "t00", "lane-pan").state.points.map((point: { tick: number }) => point.tick)).toEqual([0, 3 * BEAT]);
  expect(f.steps).toEqual(["Erase points"]);
});

test("a line bends by its handle, keeps the bend when its point moves, and a double click straightens it", () => {
  const f = panned();
  const { lanes } = editsOf(f.song);
  lanes.bend("t00", "lane-pan", 0, { dy: 0.25 });
  expect(Math.round(child(f, "t00", "lane-pan").state.points[0].curve * 100) / 100).toBe(0.5);
  // Its middle is where the handle was let go: a quarter of the way.
  expect(Math.round(laneAt(child(f, "t00", "lane-pan").state.points, BAR / 2, "linear") * 100) / 100).toBe(-0.5);
  lanes.movePoint("t00", "lane-pan", 0, { dx: 100, dy: 0 });
  expect(Math.round(child(f, "t00", "lane-pan").state.points[0].curve * 100) / 100).toBe(0.5);
  lanes.straighten("t00", "lane-pan", 100);
  expect(child(f, "t00", "lane-pan").state.points[0]).toEqual({ tick: 100, value: -1 });
  expect(f.steps).toEqual(["Bend a line", "Move a point", "Straighten a line"]);
});

test("a lane's menu moves its number with an LFO between a quarter and three quarters of its travel, and back to points at its low", () => {
  const f = fakeSong({ children: [trackOf("t00", [laneOf("lane-vol", [{ tick: 0, value: 0 }], "fader")], [{ lane: "lane-vol", target: "mix.gain_db" }])] });
  const { lanes } = editsOf(f.song);
  lanes.moveWith("t00", "lane-vol", "lfo");
  const moved = child(f, "t00", "lfo-mix-gain-db");
  expect(moved.tool).toBe("lfo");
  expect(Math.round(moved.state.low * 10) / 10).toBe(-31.3);
  expect(Math.round(moved.state.high * 10) / 10).toBe(-1.7);
  expect(child(f, "t00", "lane-vol")).toBeUndefined();
  lanes.moveWith("t00", "lfo-mix-gain-db", "lane");
  expect(child(f, "t00", "lane-mix-gain-db").state.points).toEqual([{ tick: 0, value: moved.state.low }]);
  expect(automation(f, "t00")).toEqual([{ lane: "lane-mix-gain-db", target: "mix.gain_db" }]);
  expect(f.steps).toEqual(["Move with an LFO", "Move with points"]);
});

test("an audio track has no envelope", () => {
  const f = fakeSong({ children: [tapeOf("audio-1", [laneOf("lane-vol", [{ tick: 0, value: 0 }], "fader")])] });
  (f.self.children[0] as any).state.automation = [{ lane: "lane-vol", target: "mix.gain_db" }];
  editsOf(f.song).lanes.moveWith("audio-1", "lane-vol", "envelope");
  expect(child(f, "audio-1", "lane-vol").tool).toBe("lane");
});
