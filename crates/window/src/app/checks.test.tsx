// What the test helpers of `sdk/view.ts` do, as the window does it, and what its types refuse.
import { expect, test } from "bun:test";
import { board, heard } from "./board";
import { act, find, h, input, live, memo, number, press, render, text, tool, type Card, type Self } from "./sdk";

const counter = tool({
  name: "counter", title: "Counter", when: "Never", doc: "Counts.", state: { count: number({ min: 0, max: 100, default: 0 }) }, sound: () => input,
  view: (self, { update }) => (
    <row>
      <text key="said">{self.state.count > 5 && "Many "}{self.state.count}</text>
      <box key="twice" label="Twice" onPress={() => update("Change", (draft) => { draft.state.count++; draft.state.count++; })} />
    </row>
  ),
});
const self = { name: "counter", state: { count: 0 }, children: [] } as never;

/** How often the part of `kept` was built. */
let builds = 0;
/** A counter whose buttons a memo keeps while its deps hold the same data, made new each run. */
const kept = tool({
  name: "kept", title: "Kept", when: "Never", doc: "Counts.", state: { count: number({ min: 0, max: 100, default: 0 }) }, sound: () => input,
  view: (self, ctx) => memo("buttons", [{ step: [1] }, "same"], () => (builds++,
    <row>
      <box key="add" label="Add" onPress={() => ctx.update("Add", (draft) => void (draft.state.count = self.state.count + 1))} />
      {memo("inner", [], () => <box key="deep" label="Deep" onPress={() => ctx.update("Add", (draft) => void (draft.state.count = self.state.count + 10))} />)}
      <box key="late" label="Late" onPress={() => { ctx.update("Add", (draft) => void draft.state.count++); ctx.update("Add", (draft) => void (draft.state.count = self.state.count + 1)); }} />
    </row>
  )),
});

test("a memo keeps its part while its deps hold the same data, and its handlers read the newest run", () => {
  builds = 0;
  render(kept, { name: "kept", state: { count: 0 }, children: [] });
  press("add");
  expect(press("add")).toEqual([[["state", "count"], 2]]);
  expect(press("deep")).toEqual([[["state", "count"], 12]]);
  // Four runs: built for the window once, and once more for the handlers of the newest run.
  expect(builds).toBe(3);
});

test("a handler that reads self after its own update fails: that is ctx.state", () => {
  render(kept, { name: "kept", state: { count: 0 }, children: [] });
  expect(() => press("late")).toThrow("ctx.state");
});

test("two writes to one field in one change add up", () => {
  render(counter, self);
  expect(press("twice")).toEqual([[["state", "count"], 2]]);
});

test("a text leaves out what is false", () => {
  render(counter, self);
  expect(find("said")).toMatchObject({ text: "0" });
});

const clip = { name: "board", state: { start: 480, name: "", heard: "" }, children: [] };

test("a press hears where it was in the units of its plane, once", () => {
  render(board, clip);
  press("clip", { x: 960 });
  expect(heard.at(-1)).toMatchObject({ x: 960, dx: 0, clicks: 1, right: false });
});

test("a drag lands its place on the grid unless Cmd is held, within its bounds", () => {
  render(board, clip);
  expect(act("Clip", "drag", { dx: 130 })).toEqual([[["state", "start"], 720]]);
  render(board, clip);
  expect(act("Clip", "drag", { dx: 130, cmd: true })).toEqual([[["state", "start"], 610]]);
  render(board, clip);
  expect(act("Clip", "drag", { dx: -1000 })).toEqual([[["state", "start"], 0]]);
});

const open = tool({
  name: "open", title: "Open", when: "Never", doc: "A clip with no end.", state: { start: number({ min: 0, max: 1e7, default: 0 }) }, sound: () => input,
  view: (self, { update }) => (
    <box key="lane" label="Lane" w={400} h={40} plane={{ x: 0.1 }}>
      <box key="clip" label="Clip" place={[self.state.start, 0, 960, 1]} drag={{ x: [-self.state.start, null] }} onDragEnd={(p) => update("Move", (draft) => void (draft.state.start += p.dx))} />
    </box>
  ),
});

test("a drag with one end goes as far as it likes the other way", () => {
  render(open, { name: "open", state: { start: 100 }, children: [] });
  expect(act("Clip", "drag", { dx: 5e6 })).toEqual([[["state", "start"], 5000100]]);
  expect(act("Clip", "drag", { dx: -1e7 })).toEqual([[["state", "start"], 0]]);
});

test("a drag says the key it ended over, which must be one of the view", () => {
  render(board, clip);
  act("Clip", "drag", { over: "lane" });
  expect(heard.at(-1)?.over).toBe("lane");
  expect(() => act("Clip", "drag", { over: "nowhere" })).toThrow("nowhere");
});

test("a right press opens the menu and a pick closes it; the pick's question gets its answer", async () => {
  render(board, clip, { answers: { confirm: true } });
  expect(find("Delete")).toBeUndefined();
  press("clip", { right: true });
  expect(heard.at(-1)?.right).toBe(true);
  const ops = press("Delete");
  expect(find("Delete")).toBeUndefined();
  await new Promise((done) => setTimeout(done, 0));
  expect(ops).toEqual([[["state", "start"], 0]]);
});

test("a drop hears the files, a field left with Escape commits null", () => {
  render(board, clip);
  expect(act("Lane", "drop", { audio: ["kick.wav"] })).toEqual([[["state", "name"], "kick.wav"]]);
  expect(act("Name", "commit", null)).toEqual([[["state", "name"], "left"]]);
});

test("a tool's action runs by its name; its key is what shortcut says", () => {
  render(board, clip);
  expect(find("key")).toMatchObject({ text: "h" });
  expect(act("board.home")).toEqual([[["state", "start"], 0]]);
});

const amp: Card = (self, { update }) => <box key="title" label={self.facts.title} onPress={() => update("Up", (draft) => void (draft.state.gain = 1))} />;

test("a card gets its facts, and a number a lane plays takes no change", () => {
  render(amp, { name: "amp", state: { gain: 0 }, children: [] }, { facts: { title: "Amp" }, held: ["gain"] });
  expect(find("Amp")).toBeDefined();
  expect(() => press("title")).toThrow("a lane plays gain");
});

// What the types refuse.
export const eq: Card<"eq"> = (self) => <text>{self.state.output_gain_db}</text>;
// @ts-expect-error: an eq has no cutoff
export const notEq: Card<"eq"> = (self) => <text>{self.state.cutoff_hz}</text>;
export const wrong = [
  // @ts-expect-error: a place is numbers or live values
  <box place={["left", 0]} />,
  // @ts-expect-error: what a drag plays is live
  <box key="k" label="K" drag={{ x: true, plays: { gain: 1 } }} />,
  // @ts-expect-error: a drag moves along x or y
  <box key="k" label="K" drag={{ snap: [1, 1] }} />,
  // @ts-expect-error: a shapes is placed as a box, or sized
  <shapes place={[0, 0, 10, 10]} w={10} h={10} />,
  // @ts-expect-error: a box placed as a box is sized by its place: a line N points wide is an inset
  <box place={[live.playhead, 0, 0, 4]} w={1} />,
  <box place={[live.playhead, 0, 0, 4]} inset={[0, 0, -1, 0]} />,
  <box place={[live.playhead, 0]} w={1} h={8} />,
  // A drag may have one end; a switch's value is true or false.
  <box key="k" label="K" role="switch" value={true} drag={{ x: [0, null], snap: [240, 1] }} />,
  // @ts-expect-error: a feed is one a tool publishes
  <box opacity={live.feed("peek")} />,
  // @ts-expect-error: a list feed is drawn by a shapes, not a number
  <box opacity={live.feed("response")} />,
  // @ts-expect-error: a number feed is no list to draw
  <shapes w={10} h={10} lines={[{ color: "text", width: 1, points: live.feed("peak") }]} />,
  <shapes w={10} h={10} units={[10, [18, -18]]} lines={[{ color: "text", width: 1, points: live.feed("response"), fill: "text/10" }]} />,
  <box opacity={live.feed("sounding.60")} />,
];
// A region's facts are typed: how long it plays, while its file is there.
export const seconds = (self: Self<"region">): number | null | undefined => self.facts.seconds;
