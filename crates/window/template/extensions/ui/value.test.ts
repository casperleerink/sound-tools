import { expect, test } from "bun:test";
import { fitted, shown, stepped, typedOver, typedValue } from "./value";

test("a value steps from min and stays in range, without float noise", () => {
  const range = { min: 1, max: 10, step: 4 };
  expect([fitted(0, range), fitted(12, range), fitted(6, range)]).toEqual([1, 9, 5]);
  expect(fitted(0.1 * 3, { min: 0, max: 1, step: 0.1 })).toBe(0.3);
  expect(fitted(100, { min: 0, max: 1 })).toBe(1);
});

test("an arrow moves a stepped log value by its step", () => {
  expect(stepped(20, 1, { min: 20, max: 20000, scale: "log", step: 1 })).toBe(21);
});

test("typed text is read as the value shows, and what was shown changes nothing", () => {
  expect(typedValue("25L", "pan")).toBe(-0.25);
  expect(typedValue("2 kHz", "hz")).toBe(2000);
  expect(typedValue("+3", "db")).toBe(3);
  expect(typedValue("5 x", "db")).toBeUndefined();
  const range = { min: 20, max: 20000, scale: "log" as const };
  expect(typedOver(shown(2345.6, "hz"), 2345.6, range, "hz")).toBeUndefined();
});
