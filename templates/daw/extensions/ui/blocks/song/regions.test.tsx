import { expect, test } from "bun:test";
import { act, find, h, press, render, type Self } from "../../../sdk";
import { song } from "../../../song";
import { regionOf, tapeOf, trackOf } from "../../../song/fake";
import { BAR, BEAT } from "../../../song/time";

type Song = Self<typeof song>;
const songOf = (children: unknown[]) => ({ name: "song", state: { order: [], solo: [], sidechains: [], limit_off: false }, children }) as unknown as Song;
/** A region `name` at `start` that plays `seconds` of a file `file` seconds long, or whose file is `lost`. */
const region = (name: string, start: number, seconds: number, file = seconds, lost?: "missing") => ({
  ...regionOf(name, start, seconds === file ? null : seconds),
  facts: { title: "Region", fields: [], seconds: lost ? null : seconds, file: { name: `${name}.wav`, state: lost ?? "plays", seconds: lost ? null : file } },
});
const recorder = (from: number | null) => ({ name: "rec", tool: "recorder", state: { channels: [0] }, children: [], facts: { title: "Recorder", fields: [], taking: from === null ? null : { from } } });

function opened(self: Song) {
  render((_self: { name: string; children: never[] }) => <box />, { name: "other", children: [] });
  return render(song, self);
}
const setAt = (ops: ReturnType<typeof act>, name: string) => ops.findLast(([at]) => at.at(-1) === name)?.[1];
type Placed = { place: unknown[]; lines?: { points: { node: unknown } }[]; rects?: { rects: { node: unknown } }[] };

test("an audio clip draws the wave of its file across what it plays, read by the window for what shows", () => {
  opened(songOf([tapeOf("audio-1", [region("r", BAR, 2)])]));
  // Two seconds at 120 bpm are a bar.
  const wave = find("wave-audio-1/r") as unknown as Placed;
  expect(wave.place[2]).toBe(BAR);
  expect(wave.rects?.[0]?.rects.node).toEqual({ op: "feed", name: "wave", path: "audio-1/r" });
});

test("an audio clip whose file is missing is two bars long, says so and draws no wave", () => {
  opened(songOf([tapeOf("audio-1", [region("gone", 0, 0, 0, "missing")])]));
  expect(find("gone.wav is missing")).toBeDefined();
  expect(find("wave-audio-1/gone")).toBeUndefined();
});

test("an audio clip chosen alone trims at both ends, never past its file, each one step; with another chosen it shows no handles", () => {
  opened(songOf([tapeOf("audio-1", [region("cut", BAR, 1, 2), region("b", 4 * BAR, 2)])]));
  press("clip-audio-1/cut");
  // A second of the file is left after its end: a drag far right takes it to the end.
  expect(setAt(act("End of Audio clip Cut", "drag", { dx: 4 * BAR }), "file_end_seconds")).toBe(2);
  // Its start goes no earlier than its file begins.
  expect(act("Start of Audio clip Cut", "drag", { dx: -BAR })).toEqual([]);
  const later = act("Start of Audio clip Cut", "drag", { dx: BEAT });
  expect([setAt(later, "start"), setAt(later, "file_start_seconds")]).toEqual([BAR + BEAT, 0.5]);
  press("clip-audio-1/b", { shift: true });
  expect([find("End of Audio clip Cut"), find("Fade in of Audio clip Cut")]).toEqual([undefined, undefined]);
});

test("the fade and gain handles of an audio clip play what they make while they drag, and keep it as one step each", () => {
  opened(songOf([tapeOf("audio-1", [region("r", 0, 4)])]));
  press("clip-audio-1/r");
  const handle = (label: string) => find(label) as unknown as { drag: { plays: Record<string, unknown> } };
  expect(Object.keys(handle("Fade in of Audio clip R").drag.plays)).toEqual(["audio-1/r.fade_in_ms"]);
  expect(Object.keys(handle("Gain of Audio clip R").drag.plays)).toEqual(["audio-1/r.gain_db"]);
  // A beat at 120 bpm is half a second.
  expect(setAt(act("Fade in of Audio clip R", "drag", { dx: BEAT }), "fade_in_ms")).toBe(500);
  // A fade out reaches no further than the clip plays, less its fade in.
  expect(setAt(act("Fade out of Audio clip R", "drag", { dx: -100 * BAR }), "fade_out_ms")).toBe(3500);
  // A row of drag is 23 dB; the gain stops at +24.
  expect(setAt(act("Gain of Audio clip R", "drag", { dy: -0.25 }), "gain_db")).toBe(5.8);
  expect(setAt(act("Gain of Audio clip R", "drag", { dy: -10 }), "gain_db")).toBe(24);
});

test("a take that runs draws from where it began to the playhead, its sound on an audio track and its notes on a track", () => {
  opened(songOf([tapeOf("audio-1", [recorder(BAR)]), trackOf("t00", [recorder(2 * BAR)]), tapeOf("audio-2", [recorder(null)])]));
  const take = (track: string) => find(`take-${track}`) as unknown as { place: unknown[]; children: Placed[] };
  expect(take("audio-1").place[0]).toBe(BAR);
  expect(take("audio-1").children.find((kid) => kid.rects)?.rects?.[0]?.rects.node).toEqual({ op: "feed", name: "take", path: "audio-1/rec" });
  expect(take("t00").children.find((kid) => kid.rects)?.rects?.[0]?.rects.node).toEqual({ op: "feed", name: "notes", path: "t00/rec" });
  expect(find("take-audio-2")).toBeUndefined();
});

test("an armed audio track shows the level of its input beside its name", () => {
  opened(songOf([tapeOf("audio-1", [recorder(null)])]));
  expect(find("Input level of Audio 1")).toBeUndefined();
  press("arm-audio-1");
  expect(find("Input level of Audio 1")).toMatchObject({ role: "meter" });
});
