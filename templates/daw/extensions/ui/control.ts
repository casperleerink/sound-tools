// What the controls share: their three heights, and the corner that goes with each.
import theme from "../theme.json";
import type { Color, Radius } from "../sdk";

/** 24, 28 or 32 points high: controls of one size line up in a row. A card's are `sm`. */
export type Size = "sm" | "md" | "lg";

export const height = (size: Size): number => theme.size[size];
export const corner = (size: Size): Radius => (size === "sm" ? "md" : "lg");
/** `colour` at the low opacity of a toggle that is on. */
export const tint = (colour: Color): Color => `${colour}/${theme.tint}` as Color;
