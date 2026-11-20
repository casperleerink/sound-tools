import { expect, test } from "bun:test";
import { act, defaultStates, find, h, press, render, type Card, type FieldInfo } from "../../sdk";
import { drawingOf } from "../display";
import { card } from "./cards/wavetable";

// The fields of the record as the window's facts give them, from its new state.
const choices: Record<string, (string | number)[]> = {
  table: ["basic_shapes", "vowels"], effect: ["none", "fm", "sync", "warp", "fold"], "sub.octave": [-1, -2], type: ["low_pass", "band_pass", "high_pass", "notch"],
  slope: [12, 24], character: ["clean", "warm"], routing: ["serial", "parallel", "split"], shape: ["sine", "triangle", "saw_up", "saw_down", "square", "sample_and_hold"],
  division: ["1/4", "1/8"], feel: ["straight", "dotted", "triplet"], mode: ["poly", "mono"], source: ["env_2", "key", "velocity", "mod_wheel", "lfo_1"], destination: ["filter_1_cutoff", "amp_level", "osc_1_position"],
};
const ranges: Record<string, [number, number, string?]> = {
  position: [0, 1], effect_amount: [0, 1], octave: [-3, 3], semitone: [-12, 12], detune_cents: [-50, 50], gain: [0, 1], pan: [-1, 1], voices: [1, 8], amount: [-1, 1],
  cutoff_hz: [20, 20000, "log"], resonance: [0, 1], drive_db: [0, 24], attack_seconds: [0.001, 10, "log"], decay_seconds: [0.001, 10, "log"], sustain: [0, 1], release_seconds: [0.001, 10, "log"],
  attack_curve: [0, 1], decay_curve: [0, 1], release_curve: [0, 1], rate_hz: [0.01, 40, "log"], polyphony: [1, 16], glide_seconds: [0, 5],
};
const fieldOf = (name: string, value: unknown, group = ""): FieldInfo => {
  if (Array.isArray(value)) return { name, label: name, kind: "list", item: fieldOf("", value[0]), maxItems: 8 } as FieldInfo;
  if (typeof value === "object" && value !== null) return { name, label: name, kind: "group", fields: Object.entries(value).map(([key, each]) => fieldOf(key, each, name)) } as FieldInfo;
  if (typeof value === "boolean") return { name, label: name, kind: "switch", default: value } as FieldInfo;
  const options = choices[`${group}.${name}`] ?? choices[name];
  if (options) return { name, label: name, kind: "choice", options: options.map((option) => ({ value: option, text: String(option) })), default: value } as FieldInfo;
  const [min, max, scale] = ranges[name] ?? [0, 1];
  return { name, label: name, kind: "number", min, max, scale: scale ?? "linear", integer: false, unit: null, default: value } as FieldInfo;
};
const NEW = defaultStates.wavetable as Record<string, unknown>;
const FIELDS = Object.entries(NEW).map(([name, value]) => fieldOf(name, value));
const open = (state: Record<string, unknown> = {}) => {
  render((() => <box />) as Card, { name: "other", state: {}, children: [] });
  return render(card, { name: "wavetable", state: { ...structuredClone(NEW), ...state }, children: [] } as never, { facts: { title: "Wavetable", fields: FIELDS, tables: [] } } as never);
};
const marked = (page: string) => JSON.stringify(find(`Wavetable ${page}`)).includes('"w":4');
const named = (ops: [readonly (string | number)[], unknown][]) => ops.map(([path, value]) => [path.slice(1).join("."), typeof value === "number" ? Math.round(value * 100) / 100 : value]);

test("wavetable: folded it shows the table with four knobs; a drag up the table moves the first position as one step", () => {
  open();
  for (const knob of ["Osc 1 position", "Gain", "Filter 1 cutoff", "Filter 1 resonance"]) expect(find(`Wavetable ${knob}`)).toMatchObject({ role: "slider" });
  expect(find("Wavetable Osc 2 position")).toBeUndefined();
  const { h: tall } = drawingOf(4, true);
  // The table lays a position out over 0.4 of the drawing: a fifth of it up is half the table.
  expect(named(act("Wavetable Osc 1 position on the table", "drag", { dy: -tall * 0.2 }))).toEqual([["osc_1.position", 1]]);
});

test("wavetable: its pages show one at a time, and the tabs change no record", () => {
  open();
  expect(press("More of Wavetable")).toEqual([]);
  expect(find("Wavetable Osc 2 position")).toBeDefined();
  expect(press("Wavetable Matrix")).toEqual([]);
  expect(find("Wavetable Osc 2 position")).toBeUndefined();
  expect(find("Wavetable add route")).toBeDefined();
  expect(press("Wavetable Env")).toEqual([]);
  expect(press("Wavetable envelope shown Env 2")).toEqual([]);
  expect(named(act("Wavetable Env 2 attack", "set", 0.5))).toEqual([["env_2.attack_seconds", 0.5]]);
});

test("wavetable: a page is marked by what it shows, not by what the folded card shows", () => {
  open({ osc_1: { ...(NEW.osc_1 as object), position: 0.9, table: "vowels" }, filter_1: { ...(NEW.filter_1 as object), cutoff_hz: 400 } });
  press("More of Wavetable");
  expect(["Osc", "Voice", "Filter", "Env", "LFO", "Matrix"].map(marked)).toEqual([false, false, false, false, false, false]);
  open({ osc_1: { ...(NEW.osc_1 as object), gain: 0.4 }, lfo_2: { ...(NEW.lfo_2 as object), sync: true } });
  press("More of Wavetable");
  expect(["Osc", "Voice", "Filter", "Env", "LFO", "Matrix"].map(marked)).toEqual([true, false, false, false, true, false]);
});

test("wavetable: routes are added, changed and taken out, one step each", () => {
  open();
  press("More of Wavetable");
  press("Wavetable Matrix");
  const routes = (ops: [readonly (string | number)[], unknown][]) => ops.map(([path, value]) => [path.slice(1).join("."), (value as unknown[]).length, (value as unknown[]).at(-1)]);
  expect(routes(press("Wavetable add route"))).toEqual([["matrix", 5, { source: "lfo_1", destination: "osc_1_position", amount: 0 }]]);
  expect(named(act("Wavetable Route 5 amount", "set", -0.25))).toEqual([["matrix.4.amount", -0.25]]);
  expect(routes(press("Wavetable remove route 1"))).toEqual([["matrix", 4, { source: "lfo_1", destination: "osc_1_position", amount: -0.25 }]]);
  expect(find("Wavetable Route 5 amount")).toBeUndefined();
});

test("wavetable: a drag of a filter's handle changes its cutoff and resonance as one step", () => {
  open();
  press("More of Wavetable");
  press("Wavetable Filter");
  const { w } = drawingOf(4, true);
  // A tenth across is a third of a decade.
  const ops = named(act("Wavetable Filter 1 cutoff and resonance", "drag", { dx: w / 10 }));
  expect(ops).toEqual([["filter_1.cutoff_hz", 1995.26]]);
  expect(named(press("Wavetable Filter 2"))).toEqual([["filter_2.on", true]]);
});

test("wavetable: mono plays one voice, so the number of voices goes", () => {
  open();
  press("More of Wavetable");
  press("Wavetable Voice");
  expect(find("Wavetable Voices")).toBeDefined();
  expect(named(press("Wavetable Mono"))).toEqual([["voicing.mode", "mono"]]);
  expect(find("Wavetable Voices")).toBeUndefined();
});
