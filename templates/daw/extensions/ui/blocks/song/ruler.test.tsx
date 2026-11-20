import { expect, test } from "bun:test";
import { act, find, h, press, render, type Node } from "../../../sdk";
import { fakeSong } from "../../../song/fake";
import { BAR, BEAT } from "../../../song/time";
import { pageOf } from "../../../song/view";
import { Ruler } from "./ruler";

/** The ruler of a song with no tracks, on `fake`'s clock and memory: what it changes shows there. */
function ruler(fake: ReturnType<typeof fakeSong>, keys: Record<string, string> = {}) {
  const song = Object.assign(Object.create(fake.song), { shortcut: (name: string) => keys[name] ?? null });
  const self = { name: "song", state: { order: [], solo: [], sidechains: [], limit_off: false }, children: [], facts: { title: "Song", fields: [] } };
  const draw = (_self: typeof self) => <Ruler page={pageOf(self as never, song)} />;
  render(draw, self);
}

test("a right press on the ruler offers what it can add there, and a pick adds it", () => {
  const f = fakeSong({ children: [] }, { piece: { sections: [{ bar: 1, name: "Intro" }] } });
  ruler(f, { delete_mark: "⌫" });
  press("ruler", { x: 100, right: true });
  // At bar 1 a section and the first tempo are there already, and no time signature change can be: shown, but no press.
  const offered = ["Add a time signature change", "Add a section", "Add a tempo change"].map((text) => find(text));
  expect(offered.map((item) => item !== undefined && "onPress" in item && item.onPress !== undefined)).toEqual([false, false, false]);
  expect(offered.every((item) => item !== undefined)).toBe(true);
  expect(JSON.stringify(find("Remove the section"))).toContain("⌫");
  press("ruler", { x: 2 * BAR + 100, right: true });
  act("Add a time signature change", "press");
  expect(f.piece.tempo_map.time_signatures).toEqual([{ signature: "4/4", bars: 2 }, { signature: "4/4", bars: 1 }]);
  expect(f.memory.mark).toEqual({ kind: "signature", bar: 3, tick: 2 * BAR });
  expect(f.steps).toEqual(["Add a time signature change"]);
});

test("a section is chosen and played from by a press, and its drag moves it to the bar it lands nearest", () => {
  const f = fakeSong({ children: [] }, { piece: { sections: [{ bar: 2, name: "Verse" }] } });
  ruler(f);
  press("Section Verse");
  expect([f.memory.mark, f.clock.tick]).toEqual([{ kind: "section", bar: 2, tick: BAR }, BAR]);
  act("Section Verse", "drag", { dx: 2 * BAR - 300 });
  expect(f.piece.sections).toEqual([{ bar: 4, name: "Verse" }]);
  expect(f.steps).toEqual(["Move a section"]);
});

test("the repeat's bar turns the repeat off and on, and its ends land on beats, as no step of undo", () => {
  const f = fakeSong({ children: [] });
  f.clock.setRepeat({ from: BAR, to: 2 * BAR });
  ruler(f);
  act("End of the repeat", "drag", { dx: BEAT * 1.6 });
  expect(f.clock.repeat).toEqual({ from: BAR, to: 2 * BAR + 2 * BEAT });
  act("Start of the repeat", "drag", { dx: 10 * BAR });
  expect(f.clock.repeat).toEqual({ from: 2 * BAR + BEAT, to: 2 * BAR + 2 * BEAT });
  press("Repeat");
  expect([f.clock.repeat, f.memory.repeatRange]).toEqual([null, { from: 2 * BAR + BEAT, to: 2 * BAR + 2 * BEAT }]);
  expect(find("Repeat")).toMatchObject({ value: false });
  expect(f.steps).toEqual([]);
});

test("the snap's list sets the grid that drags, nudges and new clips land on", () => {
  const f = fakeSong({ children: [] });
  ruler(f);
  act("Bar", "press");
  expect(f.memory.snap).toBe(BAR);
});

test("zoomed out, every second, fourth, eighth … bar number shows, 40 points apart; bar 1 always", () => {
  ruler(fakeSong({ children: [] }));
  // The zoom each number needs, by its bar: none for those that show however far out.
  const needs = new Map<number, unknown>();
  const walk = (node: Node | undefined) => {
    if (!node || typeof node !== "object") return;
    if ("children" in node && Array.isArray(node.children)) {
      const [only] = node.children;
      if (node.children.length === 1 && only?.t === "text" && /^\d+$/.test(only.text) && "opacity" in node) {
        const opacity = node.opacity as { node?: { op: string; b: number } } | number;
        needs.set(Number(only.text), typeof opacity === "number" ? null : opacity.node?.b);
      }
      node.children.forEach(walk);
    }
  };
  walk(find("ruler-numbers"));
  const zoomOf = (bars: number) => 40 / (bars * BAR);
  expect([1, 2, 3, 5, 9].map((bar) => needs.get(bar))).toEqual([null, zoomOf(1), zoomOf(2), zoomOf(4), null]);
});

test("the tempo lane leaves the key `tempo` to the transport's tempo", () => {
  const f = fakeSong({ children: [] });
  f.memory.tempoLane = true;
  ruler(f);
  expect(find("Tempo lane")).toBeDefined();
  expect(find("tempo")).toBeUndefined();
});
