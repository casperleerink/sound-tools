import { expect, test } from "bun:test";
import { act, find, h, press, render, type Drawn, type Node } from "../../../sdk";
import { clipOf, laneOf, tapeOf, trackOf } from "../../../song/fake";
import { actions } from "../../../song/keys";
import { memory as fresh, type Song } from "../../../song/model";
import { HEAD, laneGroups, pageOf, PLANE, TIME } from "../../../song/view";
import { LaneHeads, LaneTimes } from "./lanes";

type Self = Parameters<typeof pageOf>[0];
/** The song as the tests change it: its memory, one object as the window keeps it, and the context of the last run. */
const memory = fresh();
let last: Song | undefined;

/** The heads and the timeline of the song's page, with only the lanes in them. */
const Lanes = (self: Drawn<unknown>, ctx: Song) => {
  const song = { ...ctx, memory } as Song;
  last = song;
  const page = pageOf(self as unknown as Self, song);
  return (
    <row>
      <box key="heads" label="Tracks" w={HEAD} h={600} plane={{ y: PLANE }}>{laneGroups(page.rows).map((rows) => <LaneHeads page={page} rows={rows} />)}</box>
      <box key={PLANE} label="Timeline" w={1000} h={600} plane={TIME}>{laneGroups(page.rows).map((rows) => <LaneTimes page={page} rows={rows} />)}</box>
    </row>
  ) as Node;
};

const BAR = 3840;
const synth = { name: "synth", tool: "instrument.synth", state: { cutoff_hz: 2000 }, children: [] };
const volume = laneOf("lane-vol", [{ tick: 0, value: -12 }, { tick: BAR, value: 0, curve: 0.5 }, { tick: 2 * BAR, value: -6 }, { tick: 3 * BAR, value: -6 }], "fader");
const cutoff = laneOf("lane-cut", [{ tick: 0, value: 400 }, { tick: 2 * BAR, value: 6000 }], "log");
const lfo = { name: "lfo-cut", tool: "lfo", state: { shape: "sine", rate: "1/2", low: 500, high: 4000, scale: "log" }, children: [] };
const song = () => ({
  name: "song",
  state: { order: ["t00", "t01", "audio-1"], solo: [], sidechains: [], limit_off: false },
  children: [
    trackOf("t00", [synth, clipOf("c00", 0, BAR), volume, cutoff], [{ lane: "lane-vol", target: "mix.gain_db" }, { lane: "lane-cut", target: "synth.cutoff_hz" }]),
    trackOf("t01", [synth, lfo], [{ lane: "lfo-cut", target: "synth.cutoff_hz" }]),
    { ...tapeOf("audio-1", [structuredClone(volume)]), state: { colour: "teal", title: "", automation: [{ lane: "lane-vol", target: "mix.gain_db" }] } },
  ],
});
/** The song drawn afresh with every track's lanes showing. */
const shown = () => {
  // The volume lane of Track 1 open: its points take the pointer.
  Object.assign(memory, fresh(), { lanes: ["t00", "t01", "audio-1"], point: { track: "t00", lane: "lane-vol", ticks: [] } });
  render(Lanes as never, song() as never);
};
/** The state of the child `name` of the track `track` after the last change. */
const childOf = (track: string, name: string) => (last!.children.find((each) => each.name === track) as unknown as { children: { name: string; tool: string; state: any }[] }).children.find((each) => each.name === name);
const pointsOf = (track: string, lane: string) => childOf(track, lane)?.state.points as { tick: number; value: number; curve?: number }[];
/** Where `down` of the travel of the lane at row `top` is, in rows of the plane: 12% of a row over and under the travel. */
const yOf = (top: number, down: number) => top + 0.12 + down * 0.76;

test("a track's lanes show under it: a head that says what each moves and what plays, the points of the open one, and a row to add one", () => {
  shown();
  expect(find("lane-name-t00-lane-vol")).toMatchObject({ label: "Menu of the Volume lane of Track 1" });
  expect(find("lane-name-t00-lane-cut")).toMatchObject({ label: "Menu of the Synth · Cutoff lane of Track 1" });
  expect(JSON.stringify(find("lane-head-t00-lane-vol"))).toContain('"live":{"op":"value","name":"t00/mix.gain_db"');
  expect(find("point-t00-lane-vol-3840")).toMatchObject({ label: "Point at 2.1, 0 dB", role: "button", value: false });
  expect(find("Add a lane to Track 1")).toBeDefined();
  // A line bends from beside a chosen point, and a flat one not at all.
  expect(find("bend-t00-lane-vol-3840")).toBeUndefined();
  press("point-t00-lane-vol-7680");
  expect(find("bend-t00-lane-vol-3840")).toBeDefined();
  expect(find("bend-t00-lane-vol-7680")).toBeUndefined();
  // A lane opens by a press: then its points take the pointer.
  expect(find("point-t00-lane-cut-0")).toBeUndefined();
  press("lane-t00-lane-cut", { x: 100 * BAR });
  expect(find("point-t00-lane-cut-0")).toMatchObject({ label: "Point at 1.1, 400 Hz" });
  expect(find("point-t00-lane-vol-0")).toBeUndefined();
});

test("a lane's menu, from its name or a right press on it, moves its number with an LFO and takes the lane away; an audio track has no envelope", () => {
  shown();
  expect(find("lane-menu-t00-lane-vol-Moved by points")).toMatchObject({ value: true });
  expect(find("lane-menu-audio-1-lane-vol-Moved by an envelope")).toBeUndefined();
  act("lane-menu-t00-lane-vol-Moved by an LFO", "press");
  expect(childOf("t00", "lfo-mix-gain-db")?.tool).toBe("lfo");
  expect(find("lfo-shape-t00-lfo-mix-gain-db")).toMatchObject({ value: "Sine" });
  // Its band in time, with an edge that drags and steps.
  expect(find("band-high-t00-lfo-mix-gain-db")).toMatchObject({ role: "slider" });
  press("lane-t00-lane-cut", { right: true });
  act("lane-time-menu-t00-lane-cut-Remove lane", "press");
  expect(childOf("t00", "lane-cut")).toBeUndefined();
});

test("an LFO's head picks its shape and rate, and its band's edges drag and step by the arrows", () => {
  shown();
  act("lfo-rate-t01-lfo-cut-1/4", "press");
  expect(childOf("t01", "lfo-cut")?.state.rate).toBe("1/4");
  act("lfo-shape-t01-lfo-cut-square", "press");
  expect(childOf("t01", "lfo-cut")?.state.shape).toBe("square");
  const high = childOf("t01", "lfo-cut")?.state.high;
  act("band-high-t01-lfo-cut", "increment", 1);
  expect(childOf("t01", "lfo-cut")?.state.high).toBeGreaterThan(high);
  // Dragged down a tenth of its travel, its low is a tenth lower on a log scale: 20 Hz to 20 kHz is three decades.
  act("band-low-t01-lfo-cut", "drag", { dy: 0.076 });
  expect(Math.round(childOf("t01", "lfo-cut")?.state.low)).toBe(Math.round(500 / 10 ** 0.3));
});

test("an envelope's head opens its four times and sustain, which take a typed value", () => {
  shown();
  act("lane-menu-t00-lane-cut-Moved by an envelope", "press");
  act("env-attack_ms-t00-env-synth-cutoff-hz", "set", 50);
  expect(childOf("t00", "env-synth-cutoff-hz")?.state.attack_ms).toBe(50);
});

test("a pick from the menu of a new lane starts it where its number is, the numbers by device", () => {
  shown();
  expect(find("add-lane-t00-search")).toBeDefined();
  // What a lane moves already is not offered.
  expect(find("add-lane-t00-synth.cutoff_hz")).toBeUndefined();
  act("add-lane-t00-synth.resonance", "press");
  expect(pointsOf("t00", "lane-synth-resonance")).toEqual([{ tick: 0, value: 0.20000000298023224 }]);
});

test("a double click on a lane adds a point on the snap where it was, chosen", () => {
  shown();
  press("lane-t00-lane-vol", { x: 500, y: yOf(1, 0.5), clicks: 2 });
  expect(pointsOf("t00", "lane-vol")[1]?.tick).toBe(480);
  expect(Math.round(pointsOf("t00", "lane-vol")[1]!.value * 10) / 10).toBe(-12.6);
  expect(find("point-t00-lane-vol-480")).toMatchObject({ value: true });
});

test("a point drags on the snap and in its unit, between its neighbours; alt-click removes it", () => {
  shown();
  // A tenth of the travel down from 0 dB, on the fader's scale.
  act("point-t00-lane-vol-3840", "drag", { dx: 500, dy: 0.076 });
  expect(pointsOf("t00", "lane-vol")[1]).toMatchObject({ tick: 4320, curve: 0.5 });
  expect(pointsOf("t00", "lane-vol")[1]!.value).toBe(-3.6);
  expect(find("point-t00-lane-vol-4320")).toMatchObject({ label: "Point at 2.1.3, −3.6 dB", value: true });
  act("point-t00-lane-vol-4320", "drag", { dx: 10 * BAR });
  expect(pointsOf("t00", "lane-vol")[1]?.tick).toBe(2 * BAR - 1);
  press("point-t00-lane-vol-0", { alt: true });
  expect(pointsOf("t00", "lane-vol").map((point) => point.tick)).toEqual([2 * BAR - 1, 2 * BAR, 3 * BAR]);
});

test("a drag over a lane chooses the points it covers, they drag as one, delete takes them; an alt-drag erases", () => {
  shown();
  act("lane-t00-lane-vol", "drag", { x: 3000, dx: 6000 });
  expect([find("point-t00-lane-vol-3840"), find("point-t00-lane-vol-7680"), find("point-t00-lane-vol-0")]).toMatchObject([{ value: true }, { value: true }, { value: false }]);
  act("point-t00-lane-vol-7680", "drag", { dx: 240 });
  expect(pointsOf("t00", "lane-vol").map((point) => point.tick)).toEqual([0, 4080, 7920, 3 * BAR]);
  actions.delete_point!.run(last!);
  expect(pointsOf("t00", "lane-vol").map((point) => point.tick)).toEqual([0, 3 * BAR]);
  act("lane-t00-lane-cut", "drag", { x: -100, dx: 200, alt: true });
  expect(pointsOf("t00", "lane-cut").map((point) => point.tick)).toEqual([2 * BAR]);
});

test("a sloped line bends by its handle, and a double click on it straightens it", () => {
  shown();
  press("point-t00-lane-vol-3840");
  act("bend-t00-lane-vol-0", "drag", { dy: -0.1 });
  expect(pointsOf("t00", "lane-vol")[0]?.curve).toBeLessThan(0);
  press("bend-t00-lane-vol-3840", { clicks: 2 });
  expect(pointsOf("t00", "lane-vol")[1]).toEqual({ tick: BAR, value: 0 });
});

test("an edit of one lane draws only that lane again", () => {
  shown();
  // What a kept memo drew is the same element, not one drawn again.
  const [other, edited] = [find("lane-t00-lane-cut"), find("point-t00-lane-vol-0")];
  act("point-t00-lane-vol-3840", "drag", { dy: 0.076 });
  expect(find("lane-t00-lane-cut")).toBe(other);
  expect(find("point-t00-lane-vol-0")).not.toBe(edited);
});

test("a point says what it holds under the pointer, before it is chosen", () => {
  shown();
  expect(JSON.stringify(find("point-t00-lane-vol-0"))).toContain('"role":"tooltip"');
  expect(JSON.stringify(find("point-t00-lane-vol-0"))).toContain("−12 dB");
});
