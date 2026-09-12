import type { ToolDefinition } from "@/core/sdk";
import { tool as additive } from "./additive";
import { tool as curve } from "./curve";
import { tool as mixer } from "./mixer";
import { tool as polyrhythm } from "./polyrhythm";
import { tool as sampler } from "./sampler";

export const tools: Record<string, ToolDefinition> = Object.fromEntries(
  [polyrhythm, curve, sampler, additive, mixer].map((t) => [t.type, t]),
);

export function getTool(type: string): ToolDefinition {
  const t = tools[type];
  if (!t) throw new Error(`Unknown tool type "${type}"`);
  return t;
}
