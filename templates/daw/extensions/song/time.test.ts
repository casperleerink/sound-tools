import { expect, test } from "bun:test";
import { tempoSteps } from "../sdk";
import { barAt, barNear, barStarts, BAR, BEAT, placeOf, secondsBetween, tickOfBar, tickOfPlace, ticksAcross } from "./time";

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

test("a rit. and a fermata take the time the clock plays them in", () => {
  const time_signatures = [{ signature: "4/4", bars: 1 }];
  const rit = tempoSteps({ time_signatures, tempo_changes: [{ tick: 0, bpm: 120, to: 60 }, { tick: BAR, bpm: 60 }] });
  // An even slowing down from 120 to 60 over four beats lasts 4 ln 2 seconds.
  expect(Math.abs(secondsBetween(rit, 0, BAR) - 4 * Math.LN2)).toBeLessThan(0.002);
  const held = tempoSteps({ time_signatures, tempo_changes: [{ tick: 0, bpm: 120 }], fermatas: [{ tick: 3 * BEAT, hold: 2 }] });
  expect(secondsBetween(held, 0, BAR)).toBe(2.5);
  expect(secondsBetween(held, BAR, 2 * BAR)).toBe(2);
});

test("a bar is found by a search, not by counting the bars from the song's start each time", () => {
  let read = 0;
  const counted = runs.map((run) => new Proxy(run, { get: (target, key: keyof typeof run) => (key === "signature" && read++, target[key]) }));
  const near = 500 * BAR;
  for (let tick = near; tick < near + 100 * BEAT; tick += BEAT) {
    barAt(counted, tick);
    barNear(counted, tick);
  }
  expect(read).toBeLessThan(10);
  expect(barAt(counted, near)).toEqual(barAt(runs, near));
});
