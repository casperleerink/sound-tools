import { expect, test } from "bun:test";
import { act, find, h, press, render, type Card, type Facts, type FilledStateOfTool, type Tools } from "../../sdk";
import { drawingOf } from "../display";
import { card as eq } from "./cards/eq";
import { card as filter } from "./cards/filter";
import { card as synth } from "./cards/instrument.synth";
import { card as saturator } from "./cards/saturator";

type Drawn = { drag?: { x?: unknown; y?: unknown } };
const dragOf = (label: string) => (find(label) as Drawn | undefined)?.drag;
/** The card of `tool` on `state` again, as after an edit from outside: its memory stays, its facts are the tool's. */
const draw = <T extends keyof Tools>(card: Card<T, any>, tool: T, state: Partial<FilledStateOfTool<T>>, options: { held?: string[]; facts?: Partial<Facts<T>> } = {}) => render(card, { tool, state }, options);
/** The card opened anew: its memory as the window first gives it. */
const open: typeof draw = (card, tool, state, options) => {
  render((() => <box />) as Card, { name: "other", state: {}, children: [] });
  return draw(card, tool, state, options);
};

const bands = (): FilledStateOfTool<"eq">["bands"] => [
  { on: true, shape: "low_cut", frequency_hz: 60, gain_db: 0, q: 0.71 },
  { on: true, shape: "bell", frequency_hz: 400, gain_db: -4, q: 1.2 },
  { on: true, shape: "bell", frequency_hz: 2500, gain_db: 5, q: 1 },
  { on: false, shape: "high_shelf", frequency_hz: 9000, gain_db: 3, q: 0.71 },
];
const openEq = (held: string[] = []) => open(eq, "eq", { bands: bands() }, { held });

test("eq: a drag of a handle changes frequency and gain as one step, and chooses its band", () => {
  openEq();
  const { w, h: tall } = drawingOf(6, true);
  // A tenth across is a third of a decade up; a sixth up of ±18 dB is 6 dB.
  const ops = act("Eq point 3", "drag", { dx: w / 10, dy: -tall / 6 });
  expect(ops.map(([path]) => path.join("."))).toEqual(["state.bands.2.frequency_hz", "state.bands.2.gain_db"]);
  expect(Math.round(ops[0]![1] as number)).toBe(Math.round(2500 * 10 ** 0.3));
  expect(ops[1]![1]).toBe(11);
  expect(find("Eq Gain")).toMatchObject({ value: 11 });
});

test("eq: a press on a handle chooses its band, the knobs follow, and a typed gain is that band's", () => {
  openEq();
  expect(press("Eq point 2")).toEqual([]);
  expect(find("Eq Gain")).toMatchObject({ value: -4 });
  expect(act("Eq Gain value", "commit", "3")).toEqual([[["state", "bands", 1, "gain_db"], 3]]);
  // The keys reach a band from the strip above the drawing.
  press("Eq band shown 4");
  expect(find("Eq Freq")).toMatchObject({ value: 9000 });
});

test("eq: a cut or a notch has no gain, so its handle moves only sideways", () => {
  openEq();
  expect([dragOf("Eq point 1")?.x !== undefined, dragOf("Eq point 1")?.y]).toEqual([true, undefined]);
  expect(act("Eq point 1", "drag", { dx: 40, dy: -40 }).map(([path]) => path.at(-1))).toEqual(["frequency_hz"]);
});

test("eq: the shape follows the chosen band and an edit from outside, as one step", () => {
  openEq();
  expect(find("Eq Shape")).toMatchObject({ value: "Low cut" });
  press("Eq point 3");
  expect(find("Eq Shape")).toMatchObject({ value: "Bell" });
  expect(press("Notch")).toEqual([[["state", "bands", 2, "shape"], "notch"]]);
  const edited = bands();
  edited[2]!.shape = "high_cut";
  draw(eq, "eq", { bands: edited });
  expect(find("Eq Shape")).toMatchObject({ value: "High cut" });
});

test("eq: More shows the switch of each band and the output", () => {
  openEq();
  expect(find("Eq Output")).toBeUndefined();
  press("More of Eq");
  expect(find("Eq Output")).toBeDefined();
  expect(press("Eq Band 4")).toEqual([[["state", "bands", 3, "on"], true]]);
});

test("eq: a gain a lane holds does not drag, its frequency still does", () => {
  openEq(["bands[2].gain_db"]);
  expect([dragOf("Eq point 3")?.x !== undefined, dragOf("Eq point 3")?.y]).toEqual([true, undefined]);
});

const openFilter = (held: string[] = []) =>
  open(filter, "filter", { cutoff_hz: 1000, resonance: 0.2 }, { held, facts: { peak_db: [-3, 12] } });

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

test("saturator: the handle at the bend drags the drive, more to the left; the curve is one click", () => {
  open(saturator, "saturator", { drive_db: 6 });
  const { w } = drawingOf(4, true);
  // The drawing is 54 dB across: the bend 6 dB to the left is 6 dB more drive.
  expect(act("Saturator bend", "drag", { dx: (-w * 6) / 54 })).toEqual([[["state", "drive_db"], 12]]);
  expect(press("Saturator Curve Tube")).toEqual([[["state", "curve"], "tube"]]);
});

const openSynth = () => open(synth, "instrument.synth", { cutoff_hz: 2000, resonance: 0.2, attack_seconds: 0.005, decay_seconds: 0.01, sustain: 0.5, release_seconds: 0.3, gain: 0.15 });

test("synth: the corner after the decay drags decay and sustain as one step, by the travel of their knobs", () => {
  openSynth();
  const { w, h: tall } = drawingOf(4, true);
  // The drawing is 3.57 knob travels across and 1.25 levels up: a quarter travel is ten times the time.
  const ops = act("Synth decay and sustain", "drag", { dx: (w / 3.57) * 0.25, dy: (-tall / 1.25) * 0.2 });
  expect(ops.map(([path, value]) => [path.at(-1), Math.round((value as number) * 100) / 100])).toEqual([["decay_seconds", 0.1], ["sustain", 0.7]]);
  expect([dragOf("Synth attack point")?.y, dragOf("Synth release point")?.y]).toEqual([undefined, undefined]);
  expect(press("Synth Waveform Square")).toEqual([[["state", "waveform"], "square"]]);
});

test("synth: a handle says its time only while the knobs of the envelope are hidden", () => {
  openSynth();
  expect(JSON.stringify(find("Synth attack point"))).toContain("Attack 5 ms");
  press("More of Synth");
  expect(find("Synth Attack")).toMatchObject({ role: "slider" });
  expect(JSON.stringify(find("Synth attack point"))).not.toContain("Attack 5 ms");
});
