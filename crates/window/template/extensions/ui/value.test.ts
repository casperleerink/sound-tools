import { expect, test } from "bun:test";
import { fitted, shown, stepped, typedOver, typedValue } from "./value";

test("a value steps from min and stays in range, without float noise", () => {
  const range = { min: 1, max: 10, step: 4 };
  expect([fitted(0, range), fitted(12, range), fitted(6, range)]).toEqual([1, 9, 5]);
  expect(fitted(0.1 * 3, { min: 0, max: 1, step: 0.1 })).toBe(0.3);
  expect(fitted(100, { min: 0, max: 1 })).toBe(1);
  expect(stepped(0.05, 1, { min: 0.05, max: 1, step: 0.1 })).toBe(0.15);
  expect(fitted(3e-7, { min: 0, max: 1e-6, step: 1e-7 })).toBe(3e-7);
});

test("an arrow moves a stepped log value by its step", () => {
  expect(stepped(20, 1, { min: 20, max: 20000, scale: "log", step: 1 })).toBe(21);
});

test("typed text is read as the value shows, and what was shown changes nothing", () => {
  expect(typedValue("25L", "pan")).toBe(-0.25);
  expect(typedValue("2 kHz", "hz")).toBe(2000);
  expect(typedValue("+3", "db")).toBe(3);
  expect(typedValue("5 x", "db")).toBeUndefined();
  // A bare number is in one unit per kind; a typed unit wins.
  expect(typedOver("20", 5, { min: 1, max: 5000 }, "ms")).toBe(20);
  expect(typedOver("2", 1.5, { min: 0.001, max: 10 }, "seconds")).toBe(0.002);
  expect(typedOver("800", 2000, { min: 20, max: 20000, scale: "log" }, "hz")).toBe(800);
  expect(typedOver("1.5 s", 0.2, { min: 0.001, max: 10 }, "seconds")).toBe(1.5);
  expect(typedOver("3k", 2000, { min: 20, max: 20000, scale: "log" }, "hz")).toBe(3000);
  expect(typedValue("-25L", "pan")).toBeUndefined();
  expect(typedValue("25R", "pan")).toBe(0.25);
});

test("a pitch shows and reads as a note", () => {
  expect(shown(60, "pitch")).toBe("C4");
  expect(typedValue("C5", "pitch")).toBe(72);
  expect(typedValue("c#3", "pitch")).toBe(49);
  expect(typedValue("62", "pitch")).toBe(62);
  expect(Math.round((typedValue(shown(60.3, "pitch"), "pitch") ?? 0) * 100)).toBe(6030);
  const range = { min: 20, max: 20000, scale: "log" as const };
  expect(typedOver(shown(2345.6, "hz"), 2345.6, range, "hz")).toBeUndefined();
});
