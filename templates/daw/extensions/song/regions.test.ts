import { expect, test } from "bun:test";
import { fakeSong, regionOf, tapeOf } from "./fake";
import { regionEdits } from "./regions";
import { secondsBetween } from "./time";

/** To a thousandth: a tick is the finest a start moves. */
const near = (value: number) => Math.round(value * 1000) / 1000;
const voice = (state: object) => fakeSong({ children: [tapeOf("audio-1", [{ ...regionOf("r1", 3840, 6), state: { ...regionOf("r1", 3840, 6).state, ...state } }])] });
const region = (fake: ReturnType<typeof voice>) =>
  (fake.self.children[0] as { children: { name: string; state: Record<string, number> }[] }).children.find((child) => child.name === "r1")!.state;

test("a fade from the card stays within what plays less the other fade, one step", () => {
  const fake = voice({ fade_out_ms: 1000 });
  regionEdits(fake.song).setFade("audio-1/r1", "in", 9000, 6);
  expect([region(fake).fade_in_ms, fake.steps]).toEqual([5000, ["Change fade in"]]);
  regionEdits(fake.song).setFade("audio-1/r1", "out", -20, 6);
  expect(region(fake).fade_out_ms).toBe(0);
});

test("a start from the card plays from there in the file and the sound stays where it was in time", () => {
  const fake = voice({ file_start_seconds: 1 });
  const heard = (state: Record<string, number>) => secondsBetween([{ tick: 0, bpm: 120 }], 0, state.start!) - state.file_start_seconds!;
  const before = heard(region(fake));
  regionEdits(fake.song).setStart("audio-1/r1", 2);
  expect(near(region(fake).file_start_seconds)).toBe(near(2));
  expect(near(heard(region(fake)))).toBe(near(before));
  regionEdits(fake.song).setStart("audio-1/r1", 0.5);
  expect(near(region(fake).file_start_seconds)).toBe(near(0.5));
  expect(near(heard(region(fake)))).toBe(near(before));
  expect(fake.steps).toEqual(["Trim a region", "Trim a region"]);
});

test("a gain from the card stays in the range of its handle", () => {
  const fake = voice({});
  regionEdits(fake.song).setGain("audio-1/r1", 40);
  expect(region(fake).gain_db).toBe(24);
});

test("alt-up and alt-down step the chosen region's gain a dB, one step each; with none chosen, nothing", () => {
  const fake = voice({ gain_db: -3 });
  expect(regionEdits(fake.song).step(1)).toBe(false);
  fake.memory.selected = ["audio-1/r1"];
  expect(regionEdits(fake.song).step(1)).toBe(true);
  regionEdits(fake.song).step(-1);
  regionEdits(fake.song).step(-1);
  expect([region(fake).gain_db, fake.steps]).toEqual([-4, ["Change gain", "Change gain", "Change gain"]]);
});

/** 60 bpm for a bar, then 120. */
const slowFirst = { tempo_map: { time_signatures: [{ signature: "4/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 60 }, { tick: 3840, bpm: 120 }] } };

test("a start dragged back over a slower tempo stops where its file begins, and the sound stays where it was in time", () => {
  const fake = fakeSong({ children: [tapeOf("audio-1", [{ ...regionOf("r1", 3840, 6), state: { ...regionOf("r1", 3840, 6).state, file_start_seconds: 1 } }])] }, { piece: slowFirst });
  const heard = (state: Record<string, number>) => secondsBetween(slowFirst.tempo_map.tempo_changes, 0, state.start!) - state.file_start_seconds!;
  const before = heard(region(fake));
  regionEdits(fake.song).trimStart("audio-1/r1", { dx: -1920 });
  expect(region(fake).start).toBe(3840 - 960);
  expect(near(heard(region(fake)))).toBe(near(before));
});

test("a fade out dragged is measured at the end of the region, in the tempo there", () => {
  // 120 bpm for a bar, then 60: a region of 4 seconds from 0 ends 2 beats into the slow part.
  const piece = { tempo_map: { time_signatures: [{ signature: "4/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 120 }, { tick: 3840, bpm: 60 }] } };
  const fake = fakeSong({ children: [tapeOf("audio-1", [{ ...regionOf("r1", 0, 4), state: { ...regionOf("r1", 0, 4).state, fade_out_ms: 500 } }])] }, { piece });
  // Half a beat longer at 60 bpm is half a second more.
  regionEdits(fake.song).fadeOut("audio-1/r1", { dx: -480 }, 4);
  expect(region(fake).fade_out_ms).toBe(1000);
});
