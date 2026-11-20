import { expect, test } from "bun:test";
import { act, find, h, live, press, render, type Card, type FieldInfo } from "../sdk";
import { Choice } from "./choice";
import { decibelTicks, Display, FREQUENCY_TICKS } from "./display";
import { cardBinder } from "./field";
import { Handle } from "./handle";
import { Pad } from "./pad";
import { Readout } from "./readout";
import { Waveform } from "./waveform";

const number = (name: string, label: string, min: number, max: number, more: object = {}) =>
  ({ name, label, kind: "number", min, max, scale: "linear", integer: false, unit: null, default: min, ...more }) as FieldInfo;
const band = { name: "", label: "Band", kind: "group", fields: [
  number("frequency_hz", "Frequency", 20, 20000, { scale: "log", unit: "hz", default: 1000 }),
  number("gain_db", "Gain", -15, 15, { unit: "db", default: 0 }),
  { name: "on", label: "On", kind: "switch", default: true },
  { name: "shape", label: "Shape", kind: "choice", options: [{ value: "bell", text: "Bell" }, { value: "notch", text: "Notch" }], default: "bell" },
] } as FieldInfo;
const FIELDS = [{ name: "bands", label: "Band", kind: "list", item: band, maxItems: 4 } as FieldInfo];
const STATE = { bands: [{ frequency_hz: 1000, gain_db: 0, on: true, shape: "bell" }] };
const [W, H] = [400, 100];

/** A handle on band 1's frequency and gain, as a card binds it. */
const draw = (held: string[] = [], props: { onChange?: boolean } = {}) =>
  render(((self, ctx) => {
    const field = cardBinder(self, ctx);
    const [x, y] = [field.number("bands[0].frequency_hz"), { ...field.number("bands[0].gain_db"), travel: { min: -18, max: 18 } }];
    return <Handle id="band-1" label="Band 1" w={W} h={H} x={x} y={y} number="1"
      onChange={props.onChange ? (hz, db) => field.change("Change frequency and gain", { "bands[0].frequency_hz": hz, "bands[0].gain_db": db }) : undefined} />;
  }) as Card, { name: "eq", state: structuredClone(STATE), children: [] }, { facts: { title: "EQ", fields: FIELDS }, held });

test("a handle drags two numbers at once, as one change, and plays both while it moves", () => {
  draw([], { onChange: true });
  expect(find("Band 1")).toMatchObject({ role: "slider", value: "1 kHz · 0 dB" });
  expect(Object.keys((find("Band 1") as { drag: { plays: object } }).drag.plays)).toEqual(["bands[0].frequency_hz", "bands[0].gain_db"]);
  // A tenth of the way right on 20 Hz to 20 kHz is a third of a decade up; a sixth of the height up is 6 dB on ±18.
  const ops = act("Band 1", "drag", { dx: W / 10, dy: -H / 6 });
  expect(ops.length).toBe(2);
  expect(Math.round(ops[0]![1] as number)).toBe(Math.round(1000 * 10 ** 0.3));
  expect(ops[1]).toEqual([["state", "bands", 0, "gain_db"], 6]);
});

test("a handle stops at the ends of its travel and its numbers at their ranges", () => {
  draw();
  // Past the top of ±18 the gain is held to its own +15.
  expect(act("Band 1", "drag", { dy: -H * 2 })).toEqual([[["state", "bands", 0, "gain_db"], 15]]);
});

test("arrows nudge its first number, a double click sets both to their defaults", () => {
  draw();
  expect(act("Band 1", "increment", 1)[0]![1]).toBeGreaterThan(1000);
  render(((self, ctx) => {
    const field = cardBinder(self, ctx);
    return <Handle id="h" label="H" w={W} h={H} x={field.number("bands[0].frequency_hz")} y={field.number("bands[0].gain_db")} />;
  }) as Card, { name: "eq", state: { bands: [{ frequency_hz: 300, gain_db: 4, on: true, shape: "bell" }] }, children: [] }, { facts: { title: "EQ", fields: FIELDS } });
  expect(act("H", "reset")).toEqual([[["state", "bands", 0, "frequency_hz"], 1000], [["state", "bands", 0, "gain_db"], 0]]);
});

test("a number a lane holds does not drag, and the other one still does", () => {
  draw(["bands[0].gain_db"]);
  const drag = (find("Band 1") as { drag: { x?: unknown; y?: unknown } }).drag;
  expect([drag.x !== undefined, drag.y !== undefined]).toEqual([true, false]);
  expect(() => act("Band 1", "drag", { dy: -10 })).not.toThrow();
});

test("a card's binder reads and writes inside lists, names its steps and refuses what a lane holds", () => {
  render(((self, ctx) => {
    const field = cardBinder(self, ctx);
    const on = field.toggle("bands[0].on");
    const shape = field.choice("bands[0].shape", "Shape");
    return <col><Choice {...shape} /><box key={on.id} label={on.label} role="switch" value={on.on} onPress={on.onToggle} /></col>;
  }) as Card, { name: "eq", state: structuredClone(STATE), children: [] }, { facts: { title: "EQ", fields: FIELDS } });
  expect(find("EQ Shape")).toMatchObject({ role: "radiogroup" });
  expect(press("EQ Shape Notch")).toEqual([[["state", "bands", 0, "shape"], "notch"]]);
  expect(press("EQ On")).toEqual([[["state", "bands", 0, "on"], false]]);
});

test("a choice of many is a list", () => {
  const options = ["Low cut", "Low shelf", "Bell", "Notch", "High shelf", "High cut"].map((text) => ({ value: text, text }));
  render((() => <Choice id="shape" label="Shape" options={options} value="Bell" onChange={() => {}} />) as Card, { name: "t", state: {}, children: [] });
  expect(find("Shape")).toMatchObject({ role: "button", value: "Bell" });
});

test("a scale of decibels leaves out its ends and 0, and frequencies sit in octaves", () => {
  expect(decibelTicks([-18, 18], 6).map((tick) => tick.text)).toEqual(["−12", "−6", "+6", "+12"]);
  expect(FREQUENCY_TICKS.map((tick) => Math.round(tick.at * 1000) / 1000)).toEqual([0.233, 0.566, 0.9]);
});

test("a wave of a missing file says so in words, and one without a file says what to do", () => {
  const show = (file: Parameters<typeof Waveform>[0]["file"]) => JSON.stringify(render((() => <Display columns={6}><Waveform wave={live.feed("wave")} seconds={[0, 2]} file={file} w={200} h={100} empty="Drop a sound here" /></Display>) as Card, { name: "t", state: {}, children: [] }));
  expect(show({ name: "kick.wav", state: "missing" })).toContain("kick.wav is missing");
  expect(show(null)).toContain("Drop a sound here");
  expect(show({ name: "kick.wav", state: "plays" })).not.toContain("kick.wav");
});

test("a readout says a live value under its name, and a pad takes a dropped file and says one is missing", () => {
  let dropped: string[] = [];
  render((() => (
    <row>
      <Readout id="auto" label="Auto gain" text="Auto gain" value={live.feed("auto_gain")} unit="db" />
      <Pad id="kick" label="Kick" detail="kick.wav" missing pitch={36} onPress={() => {}} onDrop={(d) => (dropped = d.audio)} />
    </row>
  )) as Card, { name: "t", state: {}, children: [] });
  expect(find("Auto gain")).toMatchObject({ value: { node: { op: "feed", name: "auto_gain" } } });
  act("Kick", "drop", { audio: ["snare.wav"] });
  expect(dropped).toEqual(["snare.wav"]);
  expect(JSON.stringify(find("Kick"))).toContain("triangle-alert");
});
