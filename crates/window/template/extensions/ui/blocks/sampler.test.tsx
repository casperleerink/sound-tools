import { expect, test } from "bun:test";
import { act, find, h, press, render, type Card, type Facts, type FilledStateOfTool } from "../../sdk";
import { drawingOf } from "../display";
import { card } from "./cards/sampler";

const LIBRARY = [
  { id: "piano/grand", name: "Grand piano", category: "Piano", size: "1.0 GB", here: false },
  { id: "strings/violin-section", name: "Violin section", category: "Strings", size: "46 MB", here: true },
];
const open = (state: Partial<FilledStateOfTool<"sampler">>, facts: Partial<Facts<"sampler">> = {}, openFile: string | null = null) => {
  render((() => <box />) as Card, { name: "other", state: {}, children: [] });
  return render(card, { tool: "sampler", state }, { facts: { library: LIBRARY, ...facts }, answers: { openFile } });
};
const pluck: Partial<Facts<"sampler">> = { file: { name: "pluck.wav", state: "plays", seconds: 2 } };
const trimmed: Partial<FilledStateOfTool<"sampler">> = { sample: "pluck.wav", start_seconds: 0.1, end_seconds: 1.5, loop: true, loop_start_seconds: 0.4 };
const said = (label: string) => JSON.stringify(find(label));
/** What a step changed, by the name of each field. */
const changed = (ops: [readonly (string | number)[], unknown][]) => Object.fromEntries(ops.map(([path, value]) => [path.at(-1), value]));

test("sampler: without a file it asks for one, and Choose file loads the chosen file as the sample", async () => {
  open({});
  expect(said("Sampler file")).toContain("Drop an audio file here");
  press("Sampler choose file");
  await Promise.resolve();
  expect(find("Sampler instrument")).toMatchObject({ value: "Audio file…" });
  open({}, {}, "kalimba.wav");
  press("Sampler choose file");
  await Promise.resolve();
  expect(find("Sampler instrument")).toMatchObject({ value: "kalimba.wav" });
});

test("sampler: a file dropped on the display becomes the sample from its start to its end, as one step", () => {
  open(trimmed, pluck);
  expect(changed(act("Sampler file", "drop", { audio: ["bell.wav"] }))).toEqual({ sample: "bell.wav", start_seconds: 0, end_seconds: undefined, loop_start_seconds: undefined });
  // The file the record names, back under its name: nothing to change.
  open(trimmed, pluck);
  expect(act("Sampler file", "drop", { audio: ["pluck.wav"] })).toEqual([]);
});

test("sampler: a missing file says so and offers another; a library instrument has only its gain", () => {
  open({ sample: "gone.wav" }, { file: { name: "gone.wav", state: "missing" } });
  expect(said("Sampler file")).toContain("gone.wav is missing");
  expect(find("Sampler choose file")).toBeDefined();
  open({ library: "piano/grand" }, { instrument: { name: "Grand piano", license: "CC BY 4.0", attribution: null, state: "downloading", downloaded: 300e6, download: 900e6 } });
  expect(said("Sampler file")).toContain("Downloading Grand piano · 300 MB of 900 MB");
  expect([find("Sampler Gain") !== undefined, find("Sampler Root"), find("More of Sampler")]).toEqual([true, undefined, undefined]);
});

test("sampler: an instrument of the library picked from the list plays in place of the file, as one step", () => {
  open({ sample: "pluck.wav" }, pluck);
  expect(said("Sampler instrument")).toContain("46 MB · downloaded");
  expect(changed(press("Violin section"))).toEqual({ library: "strings/violin-section", sample: undefined });
});

test("sampler: the start pushes the loop start along, the end stays after the start, and none at the end of the file", () => {
  open(trimmed, pluck);
  press("More of Sampler");
  expect(changed(act("Sampler Start", "set", 0.6))).toEqual({ loop_start_seconds: 0.6, start_seconds: 0.6 });
  open(trimmed, pluck);
  press("More of Sampler");
  expect(act("Sampler End", "set", 0.05).map(([path, value]) => [path.at(-1), Math.round((value as number) * 1000) / 1000])).toEqual([["end_seconds", 0.11], ["loop_start_seconds", 0.1]]);
  expect(act("Sampler End", "set", 2).map(([path, value]) => [path.at(-1), value])).toEqual([["end_seconds", undefined]]);
});

test("sampler: a drag of the start line is one step; the corner after the decay drags decay and sustain as one", () => {
  open(trimmed, pluck);
  const { w, h: tall } = drawingOf(6);
  // A tenth across is a fifth of a second of the two-second file.
  expect(act("Sampler start line", "drag", { dx: w / 10 }).map(([path, value]) => [path.at(-1), Math.round((value as number) * 100) / 100])).toEqual([["start_seconds", 0.3]]);
  const ops = act("Sampler decay and sustain", "drag", { dx: w / 10, dy: (tall * 0.88) / 2 });
  expect(ops.map(([path, value]) => [path.at(-1), Math.round((value as number) * 100) / 100])).toEqual([["decay_seconds", 0.6], ["sustain", 0.5]]);
});

test("sampler: More shows the trims, the loop and the envelope; a switch is one step", () => {
  open({ sample: "pluck.wav" }, pluck);
  expect(find("Sampler Loop start")).toBeUndefined();
  press("More of Sampler");
  expect(find("Sampler Tune")).toMatchObject({ role: "slider" });
  expect(press("Sampler Loop")).toEqual([[["state", "loop"], true]]);
  expect(press("Sampler Reverse")).toEqual([[["state", "reverse"], true]]);
});
