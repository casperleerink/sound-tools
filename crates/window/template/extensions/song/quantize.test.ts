import { expect, test } from "bun:test";
import { legato, quantize } from "../sdk";

const note = (start: number, length = 100) => ({ start, length, pitch: 60, velocity: 100 });

test("quantize takes its strength and swing, and legato closes the gaps", () => {
  // A start 60 ticks past a line of 240 goes all the way, or half of it.
  expect(quantize([note(300)], { grid: 240 }).map((each) => each.start)).toEqual([240]);
  expect(quantize([note(300)], { grid: 240, strength: 0.5 }).map((each) => each.start)).toEqual([270]);
  // At full swing every second line is a third of the grid later: a triplet feel.
  expect(quantize([note(330)], { grid: 240, swing: 1 }).map((each) => each.start)).toEqual([320]);
  expect(legato([note(0, 50), note(0, 30), note(240, 50)]).map((each) => each.length)).toEqual([240, 240, 50]);
});
