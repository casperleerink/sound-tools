import { expect, test } from "bun:test";
import { act, defaultStates, find, h, press, render, type Card, type FilledStateOfTool } from "../../sdk";
import { drawingOf } from "../display";
import { card } from "./cards/wavetable";

/** A group the record gives is all the card sees of it, as in the window: these fill the rest. */
const NEW = defaultStates.wavetable as FilledStateOfTool<"wavetable">;
const open = (state: Partial<FilledStateOfTool<"wavetable">> = {}, held: string[] = []) => {
  render((() => <box />) as Card, { name: "other", state: {}, children: [] });
  return render(card, { tool: "wavetable", state }, { facts: { tables: [] }, held });
};

test("wavetable: a lane on a number of a group shows what it plays on its knob and its drawing, and takes no change", () => {
  open({}, ["osc_1.position"]);
  const playing = '{"op":"value","name":"osc_1.position"';
  expect(JSON.stringify(find("Wavetable Osc 1 position"))).toContain(playing);
  expect(JSON.stringify(find("Wavetable Osc 1 position on the table"))).toContain(playing);
  expect(() => act("Wavetable Osc 1 position", "set", 0.9)).toThrow("a lane plays osc_1.position");
});
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
  open({ osc_1: { ...NEW.osc_1, position: 0.9, table: "vowels" }, filter_1: { ...NEW.filter_1, cutoff_hz: 400 } });
  press("More of Wavetable");
  expect(["Osc", "Voice", "Filter", "Env", "LFO", "Matrix"].map(marked)).toEqual([false, false, false, false, false, false]);
  open({ osc_1: { ...NEW.osc_1, gain: 0.4 }, lfo_2: { ...NEW.lfo_2, sync: true } });
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
