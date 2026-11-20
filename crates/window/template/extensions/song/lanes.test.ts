import { expect, test } from "bun:test";
import { editsOf } from "./edits";
import { fakeSong, laneOf, tapeOf, trackOf } from "./fake";
import { laneAt, movablesOf } from "./lanes";
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

test("a double click adds a point on the snap, chosen, and its drag moves it between its neighbours; alt-click removes; the last takes the lane", () => {
  const f = panned();
  const { lanes } = editsOf(f.song);
  const tick = lanes.addPoint("t00", "lane-pan", { x: 500, y: 0.5, cmd: false });
  expect(tick).toBe(480);
  expect(f.memory.point).toEqual({ track: "t00", lane: "lane-pan", ticks: [480] });
  lanes.movePoint("t00", "lane-pan", tick, { dx: 240, dy: 0.25 });
  expect(child(f, "t00", "lane-pan").state.points[1]).toEqual({ tick: 720, value: -0.5 });
  expect(f.memory.point).toEqual({ track: "t00", lane: "lane-pan", ticks: [720] });
  expect(f.steps).toEqual(["Add a point", "Move a point"]);
  // A point does not pass its neighbours.
  lanes.movePoint("t00", "lane-pan", 720, { dx: 10 * BAR, dy: 0 });
  expect(child(f, "t00", "lane-pan").state.points[1].tick).toBe(BAR - 1);
  for (const point of [...child(f, "t00", "lane-pan").state.points]) lanes.pressPoint("t00", "lane-pan", point.tick, { alt: true, shift: false });
  expect(child(f, "t00", "lane-pan")).toBeUndefined();
  expect(automation(f, "t00")).toEqual([]);
  expect(f.memory.point).toEqual({ track: "t00", lane: "lane-pan", ticks: [] });
});

test("a drag over a lane chooses the points it covers, shift-click adds one, and a drag of one moves them all as one step", () => {
  const f = fakeSong({ children: [trackOf("t00", [laneOf("lane-pan", [0, 1, 2, 3].map((beat) => ({ tick: beat * BEAT, value: 0 })))], [{ lane: "lane-pan", target: "mix.pan" }])] });
  const { lanes } = editsOf(f.song);
  lanes.choosePoints("t00", "lane-pan", { x: 0.5 * BEAT, dx: BEAT, shift: false });
  lanes.pressPoint("t00", "lane-pan", 3 * BEAT, { shift: true, alt: false });
  expect(f.memory.point?.ticks).toEqual([BEAT, 3 * BEAT]);
  lanes.movePoint("t00", "lane-pan", BEAT, { dx: BEAT / 2, dy: -0.5 });
  expect(child(f, "t00", "lane-pan").state.points).toEqual([
    { tick: 0, value: 0 }, { tick: 1.5 * BEAT, value: 1 }, { tick: 2 * BEAT, value: 0 }, { tick: 3.5 * BEAT, value: 1 },
  ]);
  expect(f.memory.point?.ticks).toEqual([1.5 * BEAT, 3.5 * BEAT]);
  // Delete takes the chosen ones, as one step.
  expect(lanes.removeChosen()).toBe(true);
  expect(child(f, "t00", "lane-pan").state.points.map((point: { tick: number }) => point.tick)).toEqual([0, 2 * BEAT]);
  expect(f.steps).toEqual(["Move points", "Remove points"]);
  expect(lanes.removeChosen()).toBe(false);
});

test("moved points that land on others take their place", () => {
  const f = panned();
  const { lanes } = editsOf(f.song);
  lanes.pressPoint("t00", "lane-pan", 0, { shift: false, alt: false });
  lanes.pressPoint("t00", "lane-pan", BAR, { shift: true, alt: false });
  // Many chosen move together and stop at 0.
  lanes.movePoint("t00", "lane-pan", 0, { dx: -BAR, dy: 0 });
  expect(child(f, "t00", "lane-pan").state.points.map((point: { tick: number }) => point.tick)).toEqual([0, BAR]);
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

test("a new lane offers the track's volume and pan, then each device's numbers in the chain's order, at their values, but what a lane moves", () => {
  const chain = {
    name: "chain", tool: "chain", state: { order: ["eq", "filter"] },
    children: [
      { name: "filter", tool: "filter", state: { cutoff_hz: 500 }, children: [] },
      { name: "eq", tool: "eq", state: { bands: [{ frequency_hz: 100, gain_db: 3, q: 1 }] }, children: [] },
    ],
  };
  const row = trackOf("t00", [{ name: "synth", tool: "instrument.synth", state: {}, children: [] }, chain, laneOf("lane-pan", [{ tick: 0, value: 0 }])], [{ lane: "lane-pan", target: "mix.pan" }]);
  const offered = movablesOf(fakeSong({ children: [row] }).song.children[0]!);
  expect(offered[0]).toEqual({ target: "mix.gain_db", text: "Volume", group: "Track", value: 0 });
  expect(offered.some((each) => each.target === "mix.pan")).toBe(false);
  expect([...new Set(offered.map((each) => each.group))]).toEqual(["Track", "Synth", "Eq", "Filter"]);
  expect(offered.find((each) => each.target === "synth.cutoff_hz")).toMatchObject({ text: "Cutoff", value: 2000 });
  expect(offered.find((each) => each.target === "chain/eq.bands.0.gain_db")).toMatchObject({ text: "Bands 1 gain", value: 3 });
  expect(offered.find((each) => each.target === "chain/filter.cutoff_hz")).toMatchObject({ value: 500 });
});
