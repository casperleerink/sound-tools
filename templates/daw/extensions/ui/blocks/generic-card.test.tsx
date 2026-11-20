import { expect, test } from "bun:test";
import { act, find, h, press, render, type FieldInfo } from "../../sdk";
import { card } from "./generic-card";

const number = (name: string, label: string, min = 0, max = 1, more: object = {}) =>
  ({ name, label, kind: "number", min, max, scale: "linear", integer: false, unit: null, default: min, ...more }) as FieldInfo;
const choice = (name: string, label: string, texts: string[]) =>
  ({ name, label, kind: "choice", options: texts.map((text) => ({ value: text.toLowerCase(), text })), default: texts[0]!.toLowerCase() }) as FieldInfo;
/** The card on a record, with a memory of its own: as it opens. */
const draw = (state: Record<string, unknown>, fields: unknown[], held: string[] = []) => (
  render((_: { name: string; children: never[] }) => <box />, { name: "other", children: [] }),
  render(card, { name: "dev", state, children: [] }, { facts: { title: "Dev", fields: fields as FieldInfo[] }, held })
);

test("a number is a knob in its unit, a yes or no a switch, a few short options side by side and more a list", () => {
  draw({}, [
    number("cutoff_hz", "Cutoff", 20, 20000, { unit: "hz", scale: "log" }), number("pan", "Pan", -1, 1, { unit: "pan" }),
    { name: "sync", label: "Sync", kind: "switch", default: true }, choice("shape", "Shape", ["Saw", "Square"]), choice("feel", "Feel", ["Straight", "Dotted", "Triplet"]),
  ]);
  expect(find("Dev Cutoff")).toMatchObject({ role: "slider" });
  expect(find("Dev Sync")).toMatchObject({ role: "switch", value: true });
  expect(find("Dev Shape")).toMatchObject({ role: "radiogroup" });
  press("More of Dev");
  expect(find("Dev Feel")).toMatchObject({ role: "button" });
  expect(act("Dev Cutoff", "set", 1000)).toEqual([[["state", "cutoff_hz"], 1000]]);
});

test("six cells show, the rest behind More, which grows the card wider", () => {
  draw({}, ["a", "b", "c", "d", "e", "f", "g", "h"].map((name) => number(name, name.toUpperCase())));
  expect(["A", "F", "G"].map((label) => find(`Dev ${label}`) !== undefined)).toEqual([true, true, false]);
  press("More of Dev");
  expect(find("Dev H")).toBeDefined();
});

test("a list of records picks the item its knobs change, as the bands of an EQ", () => {
  const band = { name: "", label: "Band", kind: "group", fields: [number("gain_db", "Gain", -24, 24, { unit: "db" }), number("q", "Q", 0.1, 10)] };
  draw({ bands: [{ gain_db: 3, q: 1 }, { gain_db: -6, q: 2 }] }, [{ name: "bands", label: "Band", kind: "list", item: band, maxItems: 8 }]);
  expect(find("Dev Band 1 gain")).toMatchObject({ value: 3 });
  press("bands-1");
  expect(find("Dev Band 2 gain")).toMatchObject({ value: -6 });
  expect(act("Dev Band 2 gain", "set", 0)).toEqual([[["state", "bands", 1, "gain_db"], 0]]);
});

test("a group's fields change inside it, and a map picks its item by key", () => {
  const osc = { name: "osc", label: "Osc", kind: "group", fields: [number("level", "Level")] };
  const pads = { name: "pads", label: "Pad", kind: "map", item: { name: "", label: "", kind: "group", fields: [number("volume_db", "Volume", -60, 6)] } };
  draw({ osc: { level: 0.5 }, pads: { "36": { volume_db: -3 } } }, [osc, pads]);
  press("More of Dev");
  expect(act("Dev Osc level", "set", 0.25)).toEqual([[["state", "osc", "level"], 0.25]]);
  expect(act("Dev Pad 36 volume", "set", -6)).toEqual([[["state", "pads", "36", "volume_db"], -6]]);
});

test("a knob a lane plays takes no change", () => {
  draw({ depth: 0.2 }, [number("depth", "Depth")], ["depth"]);
  expect(() => act("Dev Depth", "set", 0.5)).toThrow();
});

test("a pattern is a row of steps: a press turns a step on or off, a drag sets how strong it plays", () => {
  const hits = { name: "hits", label: "Hits", kind: "list", maxItems: 4, pattern: true, item: number("", "", 0, 1, { default: 0.75 }) };
  draw({ hits: [1, 0, 0.5, 0] }, [hits]);
  expect(find("Dev Hits step 3")).toMatchObject({ role: "gridcell", value: true });
  expect(press("Dev Hits step 2")).toEqual([[["state", "hits", 1], 0.75]]);
  expect(press("Dev Hits step 1")).toEqual([[["state", "hits", 0], 0]]);
  expect(act("Dev Hits step 3", "set", 0.25)).toEqual([[["state", "hits", 2], 0.25]]);
});
