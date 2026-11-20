import { expect, test } from "bun:test";
import { barStarts, BEAT, placeOf, secondsBetween, tickOfBar, tickOfPlace, ticksAcross } from "./time";

const runs = [{ signature: "4/4", bars: 2 }, { signature: "6/8", bars: 1 }];

test("bars follow the time signatures, the last going on", () => {
  expect(barStarts(runs, 12 * BEAT)).toEqual([0, 4 * BEAT, 8 * BEAT, 11 * BEAT, 14 * BEAT]);
  expect(tickOfBar(runs, 5)).toBe(14 * BEAT);
});

test("a place is said as bar.beat.sixteenth, in the beat of its time signature, and read back", () => {
  expect(placeOf(runs, 4 * BEAT + BEAT + 240)).toEqual([2, 2, 2]);
  expect(placeOf(runs, 8 * BEAT + BEAT / 2)).toEqual([3, 2, 1]);
  expect(tickOfPlace(runs, "3.2")).toBe(8 * BEAT + BEAT / 2);
  expect(tickOfPlace(runs, "2.2.2")).toBe(5 * BEAT + 240);
  expect([tickOfPlace(runs, "0.1"), tickOfPlace(runs, "two")]).toEqual([null, null]);
});

test("seconds and ticks go through every tempo between", () => {
  const changes = [{ tick: 0, bpm: 120 }, { tick: 4 * BEAT, bpm: 60 }];
  expect(secondsBetween(changes, 2 * BEAT, 6 * BEAT)).toBe(3);
  expect(ticksAcross(changes, 2 * BEAT, 3)).toBe(4 * BEAT);
});
