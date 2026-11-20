import { expect, test } from "bun:test";
import { lengthAt } from "./clips";
import { editsOf } from "./edits";
import { clipOf, fakeSong, laneOf, regionOf, tapeOf, trackOf } from "./fake";
import { BAR, BEAT } from "./time";

type Self = ReturnType<typeof fakeSong>["self"];
const note = (start: number, pitch = 60) => ({ start, length: BEAT, pitch, velocity: 100 });
const row = (self: Self, name: string) => (self.children as { name: string; children: { name: string; tool: string; state: any }[]; state: any }[]).find((each) => each.name === name)!;
const items = (self: Self, name: string) => row(self, name).children.filter((child) => child.tool === "timeline" || child.tool === "region");
const starts = (self: Self, name: string) => items(self, name).map((item) => item.state.start).sort((a, b) => a - b);

test("a clip's start dragged left keeps its notes where they are in the song, as one step", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("c00", 2 * BAR, BAR, [note(BEAT)])])] });
  const { clips } = editsOf(f.song);
  clips.choose(["t00/c00"]);
  clips.resizeStart({ dx: -BAR });
  const clip = items(f.self, "t00")[0]!;
  expect(clip.state).toMatchObject({ start: BAR, length: 2 * BAR });
  expect(clip.state.notes[0].start).toBe(BAR + BEAT);
  expect(f.steps).toEqual(["Resize clips"]);
  // Later than its first note it does not go.
  clips.resizeStart({ dx: 3 * BAR });
  expect(items(f.self, "t00")[0]!.state).toMatchObject({ start: 2 * BAR + BEAT, length: 3 * BEAT });
});

test("a clip made shorter cuts what starts past its end, and is at least a step of the snap", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("c00", 0, BAR, [note(0), note(3 * BEAT)])])] });
  const { clips } = editsOf(f.song);
  clips.choose(["t00/c00"]);
  clips.resizeEnd({ dx: -2 * BEAT });
  expect(items(f.self, "t00")[0]!.state.notes).toEqual([note(0)]);
  clips.resizeEnd({ dx: -BAR });
  expect(items(f.self, "t00")[0]!.state.length).toBe(BEAT / 4);
});

test("a duplicate goes after the chosen clips and is chosen, so the next goes after it", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("c00", 0, BAR)])] });
  const { clips } = editsOf(f.song);
  clips.choose(["t00/c00"]);
  clips.command("duplicate");
  clips.command("duplicate");
  expect(starts(f.self, "t00")).toEqual([0, BAR, 2 * BAR]);
  expect(f.steps).toEqual(["Duplicate clips", "Duplicate clips"]);
});

test("clips of two tracks paste at the playhead on the chosen track and the one under it", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("c00", 0, BAR)]), trackOf("t01", [clipOf("c00", 0, BAR)]), trackOf("t02", [clipOf("c00", 0, BAR)])] });
  const { clips } = editsOf(f.song);
  clips.choose(["t00/c00", "t01/c00"]);
  clips.command("copy");
  clips.choose(["t01/c00"]);
  f.clock.tick = 2 * BAR;
  clips.paste();
  expect([starts(f.self, "t00"), starts(f.self, "t01"), starts(f.self, "t02")]).toEqual([[0], [0, 2 * BAR], [0, 2 * BAR]]);
  expect(f.steps).toEqual(["Paste clips"]);
});

test("a paste that would put a clip on an audio track changes nothing", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("c00", 0, BAR)]), tapeOf("audio-1", [regionOf("r", 0, 2)])] });
  const { clips } = editsOf(f.song);
  clips.choose(["t00/c00"]);
  clips.command("copy");
  clips.choose(["audio-1/r"]);
  clips.paste();
  expect(f.steps).toEqual([]);
});

test("a region dragged with alt is copied and the original stays; up a row it stays on its own kind", () => {
  const f = fakeSong({ children: [trackOf("t00"), tapeOf("audio-1", [regionOf("r", 0, 2)])] });
  const { clips } = editsOf(f.song);
  clips.choose(["audio-1/r"]);
  clips.move({ dx: BAR, dy: -1, alt: true });
  expect(starts(f.self, "audio-1")).toEqual([0, BAR]);
  expect(f.memory.selected).toEqual(["audio-1/r-2"]);
  expect(f.steps).toEqual(["Copy clips"]);
});

test("clips and regions are chosen, copied, pasted, nudged and deleted together", () => {
  const f = fakeSong({ state: { order: ["t00", "audio-1"] }, children: [trackOf("t00", [clipOf("c00", 0, BAR)]), tapeOf("audio-1", [regionOf("r", 0, 2)])] });
  const { clips } = editsOf(f.song);
  clips.all();
  expect(f.memory.selected).toEqual(["t00/c00", "audio-1/r"]);
  clips.command("copy");
  f.clock.tick = 2 * BAR;
  clips.paste();
  expect([starts(f.self, "t00"), starts(f.self, "audio-1")]).toEqual([[0, 2 * BAR], [0, 2 * BAR]]);
  clips.nudge(1);
  expect([starts(f.self, "t00"), starts(f.self, "audio-1")]).toEqual([[0, 2 * BAR + BEAT / 4], [0, 2 * BAR + BEAT / 4]]);
  clips.command("delete");
  expect([starts(f.self, "t00"), starts(f.self, "audio-1")]).toEqual([[0], [0]]);
  expect(f.steps).toEqual(["Paste clips", "Move clips", "Remove clips"]);
});

test("a nudge earlier stops the earliest at tick 0", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("c00", 100, BAR)])] });
  const { clips } = editsOf(f.song);
  clips.choose(["t00/c00"]);
  clips.nudge(-1);
  expect(starts(f.self, "t00")).toEqual([0]);
});

test("up and down move clips to the nearest track of their kind", () => {
  const f = fakeSong({ state: { order: ["t00", "audio-1", "t01"] }, children: [trackOf("t00", [clipOf("c00", 0, BAR)]), tapeOf("audio-1"), trackOf("t01")] });
  const { clips } = editsOf(f.song);
  clips.choose(["t00/c00"]);
  clips.toRow(1);
  expect([starts(f.self, "t00"), starts(f.self, "t01")]).toEqual([[], [0]]);
});

const volumeLane = (points: { tick: number; value: number }[]) => trackOf("t00", [clipOf("c00", BAR, BAR), laneOf("lane-vol", points, "fader")], [{ lane: "lane-vol", target: "mix.gain_db" }]);

test("a clip moved on its track takes its automation along, and the old place is a straight line", () => {
  const f = fakeSong({ children: [volumeLane([{ tick: 0, value: 0 }, { tick: BAR + BEAT, value: -12 }, { tick: 3 * BAR, value: 0 }])] });
  const { clips } = editsOf(f.song);
  clips.choose(["t00/c00"]);
  clips.move({ dx: 2 * BAR, dy: 0, alt: false });
  const points = row(f.self, "t00").children.find((child) => child.name === "lane-vol")!.state.points as { tick: number; value: number }[];
  expect(points.find((point) => point.tick === 3 * BAR + BEAT)?.value).toBe(-12);
  expect(points.some((point) => point.tick === BAR + BEAT)).toBe(false);
  expect(f.steps).toEqual(["Move clips"]);
});

test("a cut takes the automation under the clip and a paste puts it down; a delete leaves it", () => {
  const lane = [{ tick: 0, value: 0 }, { tick: BAR + BEAT, value: -12 }, { tick: 3 * BAR, value: 0 }];
  const pointsOf = (self: Self) => row(self, "t00").children.find((child) => child.name === "lane-vol")!.state.points as { tick: number; value: number }[];
  const cut = fakeSong({ children: [volumeLane(lane)] });
  const edits = editsOf(cut.song).clips;
  edits.choose(["t00/c00"]);
  edits.command("cut");
  expect(pointsOf(cut.self).some((point) => point.value === -12)).toBe(false);
  cut.clock.tick = 4 * BAR;
  edits.paste();
  expect(pointsOf(cut.self).find((point) => point.tick === 4 * BAR + BEAT)?.value).toBe(-12);
  const removed = fakeSong({ children: [volumeLane(lane)] });
  editsOf(removed.song).clips.choose(["t00/c00"]);
  editsOf(removed.song).clips.command("delete");
  expect(pointsOf(removed.self)).toEqual(lane);
});

test("a marquee chooses the clips and regions it covers, and with shift adds them", () => {
  const f = fakeSong({ state: { order: ["t00", "t01", "audio-1"] }, children: [trackOf("t00", [clipOf("a", 0, BAR), clipOf("b", 4 * BAR, BAR)]), trackOf("t01", [clipOf("c", BAR, BAR)]), tapeOf("audio-1", [regionOf("r", 0, 2)])] });
  const { clips } = editsOf(f.song);
  clips.marquee({ x: BAR / 2, y: 0.5, dx: BAR, dy: 1, shift: false, cmd: false });
  expect(f.memory.selected).toEqual(["t00/a", "t01/c"]);
  clips.marquee({ x: 0, y: 2.5, dx: 10, dy: 0.1, shift: true, cmd: false });
  expect(f.memory.selected).toEqual(["t00/a", "t01/c", "audio-1/r"]);
});

test("a double click adds a clip as long as its bar, in the cell under the pointer, chosen", () => {
  const f = fakeSong({ children: [trackOf("t00")] }, { piece: { tempo_map: { time_signatures: [{ signature: "3/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 120 }] } } });
  const { clips } = editsOf(f.song);
  clips.add({ x: 3 * BEAT + 100, y: 0.5 });
  expect(items(f.self, "t00")[0]!.state).toMatchObject({ start: 3 * BEAT, length: 3 * BEAT });
  expect(f.memory.selected).toEqual(["t00/clip"]);
  expect(f.steps).toEqual(["Add a clip"]);
});

test("shift-C adds a clip at the playhead as long as the bar it is in", () => {
  const f = fakeSong({ children: [trackOf("t00")] }, { tick: BAR + 300, piece: { tempo_map: { time_signatures: [{ signature: "4/4", bars: 1 }, { signature: "6/8", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 120 }] } } });
  f.memory.track = "t00";
  expect(editsOf(f.song).clips.addAtPlayhead()).toBe(true);
  expect(items(f.self, "t00")[0]!.state).toMatchObject({ start: BAR + 240, length: 3 * BEAT });
});

test("alt-right chooses the next clip on its track, and the first while none is", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("b", BAR, BAR), clipOf("a", 0, BAR)])] });
  const { clips } = editsOf(f.song);
  clips.step(1);
  expect(f.memory.selected).toEqual(["t00/a"]);
  clips.step(1);
  expect(f.memory.selected).toEqual(["t00/b"]);
});

test("audio dropped lands at the nearest beat on the audio track under it, chosen", () => {
  const f = fakeSong({ state: { order: ["t00", "audio-1"] }, children: [trackOf("t00"), tapeOf("audio-1")] });
  editsOf(f.song).clips.dropAudio(["kick.wav"], { x: BEAT + 400, y: 0.5 });
  expect(items(f.self, "audio-1")[0]!.state).toMatchObject({ asset: "kick.wav", start: BEAT });
  expect(f.memory.selected).toEqual(["audio-1/region"]);
  expect(f.steps).toEqual(["Add audio"]);
});

test("a region is as long as its file plays at the tempo where it is", () => {
  const f = fakeSong({ children: [tapeOf("audio-1", [{ ...regionOf("r", 4 * BEAT), facts: { seconds: 2 } }])] }, { piece: { tempo_map: { time_signatures: [], tempo_changes: [{ tick: 0, bpm: 120 }, { tick: 4 * BEAT, bpm: 60 }] } } });
  expect(lengthAt(f.song, "audio-1/r")).toBe(2 * BEAT);
});

test("choosing a clip of another track moves the panel there, and an open editor to that clip", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("a", 0, BAR)]), trackOf("t01", [clipOf("b", 0, BAR)])] });
  const { clips } = editsOf(f.song);
  f.memory.below = { kind: "track", name: "t00" };
  clips.choose(["t01/b"]);
  expect(f.memory.below).toEqual({ kind: "track", name: "t01" });
  clips.open("t01/b");
  clips.choose(["t00/a"]);
  expect(f.memory.below).toEqual({ kind: "editor", clip: "t00/a" });
  expect(f.selected.at(-1)).toBe("t00/a");
});
