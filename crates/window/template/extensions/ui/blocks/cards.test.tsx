import { expect, test } from "bun:test";
import { act, find, h, press, render, type Card, type FieldInfo, type Node } from "../../sdk";
import { drawingOf } from "../display";
import { card as eq } from "./cards/eq";
import { card as filter } from "./cards/filter";
import { card as synth } from "./cards/instrument.synth";
import { card as saturator } from "./cards/saturator";

// The cards' fields as the window's facts give them.
const num = (name: string, min: number, max: number, value: number, more: object = {}) =>
  ({ name, label: name, kind: "number", min, max, scale: "linear", integer: false, unit: null, default: value, ...more }) as FieldInfo;
const choice = (name: string, values: (string | number)[]) =>
  ({ name, label: name, kind: "choice", options: values.map((value) => ({ value, text: String(value) })), default: values[0]! }) as FieldInfo;
const log = { scale: "log" };

type Drawn = { drag?: { x?: unknown; y?: unknown } };
const dragOf = (label: string) => (find(label) as Drawn | undefined)?.drag;
/** The card on `state`, opened anew: its memory as the window first gives it. */
const open = (card: (self: any, ctx: any) => Node, title: string, fields: FieldInfo[], state: object, more: { held?: string[]; facts?: object } = {}) => {
  render((() => <box />) as Card, { name: "other", state: {}, children: [] });
  return draw(card, title, fields, state, more);
};
/** The card on `state` again, as after an edit from outside: its memory stays. */
const draw = (card: (self: any, ctx: any) => Node, title: string, fields: FieldInfo[], state: object, { held = [], facts = {} }: { held?: string[]; facts?: object } = {}) =>
  render(card, { name: title.toLowerCase(), state, children: [] } as never, { facts: { title, fields, ...facts }, held } as never);

const BAND = { name: "", label: "Band", kind: "group", fields: [
  { name: "on", label: "On", kind: "switch", default: true },
  choice("shape", ["low_cut", "low_shelf", "bell", "notch", "high_shelf", "high_cut"]),
  num("frequency_hz", 20, 20000, 1000, { ...log, unit: "hz" }), num("gain_db", -15, 15, 0, { unit: "db" }), num("q", 0.1, 18, 0.71, log),
] } as FieldInfo;
const EQ = [{ name: "bands", label: "Bands", kind: "list", item: BAND, maxItems: 4 } as FieldInfo, num("output_gain_db", -12, 12, 0, { unit: "db" })];
const bands = () => [
  { on: true, shape: "low_cut", frequency_hz: 60, gain_db: 0, q: 0.71 },
  { on: true, shape: "bell", frequency_hz: 400, gain_db: -4, q: 1.2 },
  { on: true, shape: "bell", frequency_hz: 2500, gain_db: 5, q: 1 },
  { on: false, shape: "high_shelf", frequency_hz: 9000, gain_db: 3, q: 0.71 },
];
const openEq = (held: string[] = []) => open(eq, "EQ", EQ, { bands: bands(), output_gain_db: 0 }, { held });

test("eq: a drag of a handle changes frequency and gain as one step, and chooses its band", () => {
  openEq();
  const { w, h: tall } = drawingOf(6, true);
  // A tenth across is a third of a decade up; a sixth up of ±18 dB is 6 dB.
  const ops = act("EQ point 3", "drag", { dx: w / 10, dy: -tall / 6 });
  expect(ops.map(([path]) => path.join("."))).toEqual(["state.bands.2.frequency_hz", "state.bands.2.gain_db"]);
  expect(Math.round(ops[0]![1] as number)).toBe(Math.round(2500 * 10 ** 0.3));
  expect(ops[1]![1]).toBe(11);
  expect(find("EQ Gain")).toMatchObject({ value: 11 });
});

test("eq: a press on a handle chooses its band, the knobs follow, and a typed gain is that band's", () => {
  openEq();
  expect(press("EQ point 2")).toEqual([]);
  expect(find("EQ Gain")).toMatchObject({ value: -4 });
  expect(act("EQ Gain value", "commit", "3")).toEqual([[["state", "bands", 1, "gain_db"], 3]]);
  // The keys reach a band from the strip above the drawing.
  press("EQ band shown 4");
  expect(find("EQ Freq")).toMatchObject({ value: 9000 });
});

test("eq: a cut or a notch has no gain, so its handle moves only sideways", () => {
  openEq();
  expect([dragOf("EQ point 1")?.x !== undefined, dragOf("EQ point 1")?.y]).toEqual([true, undefined]);
  expect(act("EQ point 1", "drag", { dx: 40, dy: -40 }).map(([path]) => path.at(-1))).toEqual(["frequency_hz"]);
});

test("eq: the shape follows the chosen band and an edit from outside, as one step", () => {
  openEq();
  expect(find("EQ Shape")).toMatchObject({ value: "low_cut" });
  press("EQ point 3");
  expect(find("EQ Shape")).toMatchObject({ value: "bell" });
  expect(press("notch")).toEqual([[["state", "bands", 2, "shape"], "notch"]]);
  const edited = bands();
  edited[2]!.shape = "high_cut";
  draw(eq, "EQ", EQ, { bands: edited, output_gain_db: 0 });
  expect(find("EQ Shape")).toMatchObject({ value: "high_cut" });
});

test("eq: More shows the switch of each band and the output", () => {
  openEq();
  expect(find("EQ Output")).toBeUndefined();
  press("More of EQ");
  expect(find("EQ Output")).toBeDefined();
  expect(press("EQ Band 4")).toEqual([[["state", "bands", 3, "on"], true]]);
});

test("eq: a gain a lane holds does not drag, its frequency still does", () => {
  openEq(["bands[2].gain_db"]);
  expect([dragOf("EQ point 3")?.x !== undefined, dragOf("EQ point 3")?.y]).toEqual([true, undefined]);
});

const FILTER = [
  choice("type", ["low_pass", "band_pass", "high_pass", "notch"]), num("cutoff_hz", 20, 20000, 1000, { ...log, unit: "hz" }), num("resonance", 0, 1, 0.2), choice("slope", [12, 24]),
  num("drive_db", 0, 24, 0, { unit: "db" }), choice("character", ["clean", "warm"]), num("mix", 0, 1, 1), num("lfo_rate_hz", 0.05, 20, 1, { ...log, unit: "hz" }), num("lfo_depth_octaves", 0, 4, 0),
];
const openFilter = (held: string[] = []) =>
  open(filter, "Filter", FILTER, { type: "low_pass", cutoff_hz: 1000, resonance: 0.2, slope: 12, drive_db: 0, character: "clean", mix: 1, lfo_rate_hz: 1, lfo_depth_octaves: 0 }, { held, facts: { peak_db: [-3, 12] } });

test("filter: a drag of the handle changes cutoff and resonance as one step, its height the resonance on the peak", () => {
  openFilter();
  const { w, h: tall } = drawingOf(4, true);
  // The peak goes from -3 to 12 dB over the resonance, on a drawing of 18 to -36 dB: 3.6 resonances tall.
  const ops = act("Filter cutoff and resonance", "drag", { dx: w / 10, dy: (-tall * 0.3) / 3.6 });
  expect(ops.map(([path, value]) => [path.at(-1), Math.round((value as number) * 100) / 100])).toEqual([["cutoff_hz", 1995.26], ["resonance", 0.5]]);
});

test("filter: the type is one click and one step; More shows the slope and the LFO", () => {
  openFilter();
  expect(press("Filter Type Band")).toEqual([[["state", "type"], "band_pass"]]);
  expect(find("Filter LFO rate")).toBeUndefined();
  press("More of Filter");
  expect(find("Filter LFO depth")).toMatchObject({ role: "slider" });
  expect(press("Filter Slope 24")).toEqual([[["state", "slope"], 24]]);
});

test("filter: a value typed on a knob is read in its unit; a cutoff a lane holds takes nothing and the handle only rises", () => {
  openFilter();
  expect(act("Filter Cutoff value", "commit", "2k")).toEqual([[["state", "cutoff_hz"], 2000]]);
  openFilter(["cutoff_hz"]);
  expect(() => act("Filter Cutoff", "set", 500)).toThrow();
  expect([dragOf("Filter cutoff and resonance")?.x, dragOf("Filter cutoff and resonance")?.y !== undefined]).toEqual([undefined, true]);
});

const SATURATOR = [choice("curve", ["soft", "tape", "tube", "clip"]), num("drive_db", 0, 36, 6, { unit: "db" }), num("tone_db", -12, 12, 0, { unit: "db" }), num("output_db", -12, 12, 0, { unit: "db" }), num("mix", 0, 1, 1)];

test("saturator: the handle at the bend drags the drive, more to the left; the curve is one click", () => {
  open(saturator, "Saturator", SATURATOR, { curve: "soft", drive_db: 6, tone_db: 0, output_db: 0, mix: 1 });
  const { w } = drawingOf(4, true);
  // The drawing is 54 dB across: the bend 6 dB to the left is 6 dB more drive.
  expect(act("Saturator bend", "drag", { dx: (-w * 6) / 54 })).toEqual([[["state", "drive_db"], 12]]);
  expect(press("Saturator Curve tube")).toEqual([[["state", "curve"], "tube"]]);
});

const SYNTH = [
  choice("waveform", ["saw", "square"]), num("cutoff_hz", 20, 20000, 2000, { ...log, unit: "hz" }), num("resonance", 0, 1, 0.2), choice("character", ["clean", "warm"]),
  num("attack_seconds", 0.001, 10, 0.005, log), num("decay_seconds", 0.001, 10, 0.2, log), num("sustain", 0, 1, 0.7), num("release_seconds", 0.001, 10, 0.3, log), num("gain", 0, 1, 0.15),
];
const openSynth = () => open(synth, "Synth", SYNTH, { waveform: "saw", cutoff_hz: 2000, resonance: 0.2, character: "clean", attack_seconds: 0.005, decay_seconds: 0.01, sustain: 0.5, release_seconds: 0.3, gain: 0.15 });

test("synth: the corner after the decay drags decay and sustain as one step, by the travel of their knobs", () => {
  openSynth();
  const { w, h: tall } = drawingOf(4, true);
  // The drawing is 3.57 knob travels across and 1.25 levels up: a quarter travel is ten times the time.
  const ops = act("Synth decay and sustain", "drag", { dx: (w / 3.57) * 0.25, dy: (-tall / 1.25) * 0.2 });
  expect(ops.map(([path, value]) => [path.at(-1), Math.round((value as number) * 100) / 100])).toEqual([["decay_seconds", 0.1], ["sustain", 0.7]]);
  expect([dragOf("Synth attack point")?.y, dragOf("Synth release point")?.y]).toEqual([undefined, undefined]);
  expect(press("Synth Waveform square")).toEqual([[["state", "waveform"], "square"]]);
});

test("synth: a handle says its time only while the knobs of the envelope are hidden", () => {
  openSynth();
  expect(JSON.stringify(find("Synth attack point"))).toContain("Attack 5 ms");
  press("More of Synth");
  expect(find("Synth Attack")).toMatchObject({ role: "slider" });
  expect(JSON.stringify(find("Synth attack point"))).not.toContain("Attack 5 ms");
});
