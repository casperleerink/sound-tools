import { expect, test } from "bun:test";
import { act, find, h, press, render, type Card, type Facts } from "../../sdk";
import { card } from "./cards/plugin";

// The parameters of the repo's test CLAP plugin, as the facts list them.
const PARAMETERS = [
  { id: 0, name: "Cutoff", min: 20, max: 20000, default: 1000, steps: null, names: [], automatable: true, lane: true },
  { id: 1, name: "Wave", min: 0, max: 2, default: 0, steps: 3, names: [{ value: 0, name: "Sine" }, { value: 1, name: "Saw" }, { value: 2, name: "Square" }], automatable: false, lane: false },
  { id: 5, name: "Bright", min: 0, max: 1, default: 0, steps: 2, names: [], automatable: true, lane: false },
];
const open = (parameters: Record<string, { name: string; value: number }> = {}, facts: Partial<Facts<"plugin">> = {}, held: string[] = []) => {
  render((() => <box />) as Card, { name: "other", state: {}, children: [] });
  const state = { format: "clap", plugin_id: "sound-tools.test-tone", state_asset: "tone", parameters } as const;
  return render(card, { tool: "plugin", state }, { facts: { title: "Test Tone", name: "Test Tone", status: "plays", window: "closed", parameters: PARAMETERS, texts: {}, ...facts }, held });
};
const said = (label: string) => JSON.stringify(find(label));

test("plugin: a parameter pinned from the list turns on the card in the plugin's words; taking it off is one step", () => {
  open();
  // The plugin pins it at what it plays now, which only it knows: the card asks, and changes no record itself.
  expect(press("Cutoff")).toEqual([]);
  open({ 0: { name: "Cutoff", value: 1000 } }, { texts: { 0: "1000 Hz" } });
  expect(find("Test Tone Cutoff")).toMatchObject({ role: "slider", value: 1000 });
  expect(said("Test Tone Cutoff")).toContain("1000 Hz");
  expect(act("Test Tone Cutoff", "set", 2000)).toEqual([[["state", "parameters", "0", "value"], 2000]]);
  expect(press("Cutoff")).toEqual([[["state", "parameters", "0"], undefined]]);
  expect(find("Test Tone Cutoff")).toBeUndefined();
});

test("plugin: two steps are a switch and named steps a list, each setting the value of its step", () => {
  open({ 1: { name: "Wave", value: 0 }, 5: { name: "Bright", value: 0 } });
  expect(press("Test Tone Bright")).toEqual([[["state", "parameters", "5", "value"], 1]]);
  expect(find("Test Tone Wave")).toMatchObject({ value: "Sine" });
  expect(press("Square")).toEqual([[["state", "parameters", "1", "value"], 2]]);
});

test("plugin: a pin the plugin lacks says so and comes off from the list; one outside the range says that", () => {
  open({ 9: { name: "Gone", value: 0.5 }, 0: { name: "Cutoff", value: 50000 } });
  expect(said("Test Tone Gone not found")).toContain("This plugin has no parameter with the id 9");
  expect(said("Test Tone parameters")).toContain("Not in this plugin");
  expect(press("Gone")).toEqual([[["state", "parameters", "9"], undefined]]);
  expect(JSON.stringify(find("card"))).toContain("Out of range");
});

test("plugin: a pin a lane moves cannot come off, and the window button names what it does", () => {
  open({ 0: { name: "Cutoff", value: 1000 } }, {}, ["parameters.0.value"]);
  expect(said("Test Tone parameters")).toContain("Automated. Delete its lane to take it off");
  expect(() => press("Cutoff")).toThrow("has no press");
  expect(find("Test Tone open window")).toBeDefined();
  open({}, { window: "open" });
  expect(find("Test Tone close window")).toBeDefined();
  open({}, { window: "none" });
  expect(JSON.stringify(find("card"))).toContain("This plugin has no window.");
});

test("plugin: one that cannot play says why where the display would be", () => {
  open({}, { status: "missing", name: null, parameters: [] });
  expect(JSON.stringify(find("card"))).toContain("This plugin is not on this");
  open({}, { status: "stopped", parameters: [] });
  expect(JSON.stringify(find("card"))).toContain("Test Tone stopped working.");
  expect(find("Test Tone start again")).toBeDefined();
  open({}, { status: "not_loaded", parameters: [] });
  expect(find("Test Tone parameters")).toBeUndefined();
});
