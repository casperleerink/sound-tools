// What the test helpers of `sdk/view.ts` do, as the window does it, and what its types refuse.
import { expect, test } from "bun:test";
import { act, find, h, input, live, number, press, render, text, tool, type Card, type Pointer } from "./sdk";

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

test("two writes to one field in one change add up", () => {
  render(counter, self);
  expect(press("twice")).toEqual([[["state", "count"], 2]]);
});

test("a text leaves out what is false", () => {
  render(counter, self);
  expect(find("said")).toMatchObject({ text: "0" });
});

/** What the handlers of the clip heard. */
const heard: Pointer[] = [];
const board = tool({
  name: "board", title: "Board", when: "Never", doc: "A clip in time.", sound: () => input,
  state: { start: number({ min: 0, max: 100000, default: 480 }), name: text({ default: "" }) },
  actions: { home: { title: "Back to the start", keys: ["h"], run: ({ update }) => update("Home", (draft) => void (draft.state.start = 0)) } },
  view: (self, { update, confirm, shortcut }) => (
    <box key="lane" label="Lane" w={400} h={40} plane={{ x: 0.1 }} onDrop={(d) => update("Drop", (draft) => void (draft.state.name = d.audio[0] ?? ""))}>
      <box key="clip" label="Clip" place={[live.drag("clip").dx.plus(self.state.start), 0, 960, 40]} drag={{ x: [-self.state.start, 100000], snap: [240, 1] }}
        onPress={(p) => heard.push(p)} onDragEnd={(p) => { heard.push(p); update("Move", (draft) => void (draft.state.start += p.dx)); }}>
        <col overlay={{ open: "menu" }} role="menu">
          <box key="delete" label="Delete" onPress={async () => { if (await confirm({ message: "Delete the clip?", confirm: "Delete" })) update("Delete", (draft) => void (draft.state.start = 0)); }} />
        </col>
      </box>
      <text key="key">{shortcut("home") ?? "none"}</text>
      <field key="name" label="Name" value={self.state.name} onCommit={(name) => update("Name", (draft) => void (draft.state.name = name ?? "left"))} />
    </box>
  ),
});
const clip = { name: "board", state: { start: 480, name: "" }, children: [] };

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
  // @ts-expect-error: a feed is one a tool publishes
  <box opacity={live.feed("peek")} />,
];
