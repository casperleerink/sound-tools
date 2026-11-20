import { expect, test } from "bun:test";
import { act, find, h, press, render, type Card, type Facts } from "../../sdk";
import { card } from "./cards/drum-pad";

// The facts as the window gives them: the kit's pads are in no record.
const KIT = ["Kick", "Rim", "Snare", "Clap", "Snare 2", "Tom 1", "Hat", "Tom 2", "Pedal hat", "Tom 3", "Open hat", "Tom 4", "Tom 5", "Crash", "Tom 6", "Ride"];
const kit = { volume_db: 0, pitch_semitones: 0, decay_ms: 300, pan: 0 };
const padsOf = (record: Record<string, Record<string, unknown>>): Facts<"drum-pad">["pads"] =>
  KIT.map((name, index) => {
    const own = record[String(36 + index)] ?? {};
    const sample = own.sample as string | undefined;
    return {
      note: 36 + index, name: sample ? sample.replace(/\.[a-z]+$/, "") : name, sound: sample ? null : name.toLowerCase(),
      file: sample ? { name: sample, state: sample === "gone.wav" ? "missing" : "plays", seconds: 1 } : null,
      ...kit, decay_ms: index === 1 ? 70 : 300, choke: index === 6, ...own, kit: { ...kit, decay_ms: index === 1 ? 70 : 300 },
    };
  });
const SOUNDS: Facts<"drum-pad">["sounds"] = ["kick", "rim", "snare", "clap", "hat"].map((value) => ({ value, name: value.charAt(0).toUpperCase() + value.slice(1) }));
const open = (record: Record<string, Record<string, unknown>> = {}, held: string[] = []) => {
  render((() => <box />) as Card, { name: "other", state: {}, children: [] });
  return render(card, { tool: "drum-pad", state: { pads: record } }, { facts: { pads: padsOf(record), sounds: SOUNDS }, held });
};

test("drum pad: sixteen pads named for what they play, the first at the bottom left; each says a file it plays or misses", () => {
  open({ "37": { sample: "shaker.wav" }, "40": { sample: "gone.wav" } });
  const pads = JSON.stringify(find("Drum pad pads"));
  expect(pads.indexOf("Tom 5")).toBeLessThan(pads.indexOf("Kick"));
  expect(JSON.stringify(find("Drum pad pad 2"))).toContain("audio-lines");
  expect(JSON.stringify(find("Drum pad pad 5"))).toContain("triangle-alert");
  expect(JSON.stringify(find("Drum pad pad 3"))).not.toContain("icon");
});

test("drum pad: a press chooses the pad and plays it, and is no edit; the knobs show that pad as it plays", () => {
  open();
  expect(press("Drum pad pad 2")).toEqual([]);
  expect(find("Drum pad Decay")).toMatchObject({ value: 70 });
  expect(find("Drum pad sound")).toMatchObject({ value: "Rim" });
});

test("drum pad: a knob changes the chosen pad alone, as one step, and a reset gives it its kit's value", () => {
  open({ "37": { decay_ms: 200 } });
  press("Drum pad pad 2");
  expect(act("Drum pad Volume", "set", -6)).toEqual([[["state", "pads", "37", "volume_db"], -6]]);
  expect(act("Drum pad Decay", "reset")).toEqual([[["state", "pads", "37", "decay_ms"], 70]]);
  press("Drum pad pad 3");
  expect(act("Drum pad Pan", "set", 0.5).map(([path]) => path.join("."))).toEqual(["state.pads.38"]);
});

test("drum pad: a sound picked from the list plays in place of the pad's file", () => {
  open({ "36": { sample: "kick.wav" } });
  expect(find("Drum pad sound")).toMatchObject({ value: "kick.wav" });
  const ops = press("Hat");
  expect(ops.map(([path, value]) => [path.join("."), value])).toEqual([["state.pads.36.sample", undefined], ["state.pads.36.sound", "hat"]]);
});

test("drum pad: a file dropped on a pad makes it play the file, as one step, and chooses it", () => {
  open();
  const ops = act("Drum pad pad 4", "drop", { audio: ["clap.wav"] });
  expect(ops).toEqual([[["state", "pads", "39"], { sample: "clap.wav", pitch_semitones: 0, decay_ms: 10000 }]]);
  expect(find("Drum pad sound")).toMatchObject({ value: "clap.wav" });
  expect(act("Drum pad pad 5", "drop", { audio: [] })).toEqual([]);
});

test("drum pad: Choose file in the list loads the chosen file on the chosen pad", async () => {
  render((() => <box />) as Card, { name: "other", state: {}, children: [] });
  render(card, { tool: "drum-pad" }, { facts: { pads: padsOf({}), sounds: SOUNDS }, answers: { openFile: "loop.wav" } });
  press("Drum pad pad 3");
  press("Choose file…");
  await Promise.resolve();
  expect(find("Drum pad sound")).toMatchObject({ value: "loop.wav" });
});

test("drum pad: More puts the chosen pad in the choke group or takes it out, and resets it to the kit", () => {
  open({ "36": { volume_db: -3 } });
  expect(find("Drum pad choke")).toBeUndefined();
  press("More of Drum pad");
  expect(press("Drum pad choke")).toEqual([[["state", "pads", "36", "choke"], true]]);
  expect(press("Drum pad reset pad")).toEqual([[["state", "pads", "36"], undefined]]);
});

test("drum pad: a volume a lane holds shows what plays; the other numbers still turn", () => {
  open({}, ["pads.36.volume_db"]);
  expect(typeof (find("Drum pad Volume") as { value?: unknown }).value).toBe("object");
  expect(act("Drum pad Pitch", "set", 2)).toEqual([[["state", "pads", "36"], { pitch_semitones: 2 }]]);
});
