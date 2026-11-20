import { expect, test } from "bun:test";
import { keysOf } from "./kbd";

test("a key hint draws held keys in the Mac's order, as the app's menus do, and reads a minus key", () => {
  expect([keysOf("cmd-shift-z"), keysOf("alt-shift-right"), keysOf("cmd--"), keysOf(null)]).toEqual(["⇧⌘Z", "⌥⇧→", "⌘-", undefined]);
});
