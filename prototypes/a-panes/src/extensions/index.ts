import type { ExtensionDef, ToolDef, ToolInstance } from "@/sdk";
import { toneExtension } from "./tone";
import { tremoloExtension } from "./tremolo";
import { loopsExtension } from "./loops";
import { latticeExtension } from "./lattice";
import { mixerExtension } from "./mixer";

export const EXTENSIONS: ExtensionDef[] = [toneExtension, tremoloExtension, loopsExtension, latticeExtension, mixerExtension];

export function toolFor(instance: ToolInstance): ToolDef {
  for (const ext of EXTENSIONS) {
    if (ext.name !== instance.extension) continue;
    const tool = ext.tools.find((t) => t.id === instance.tool);
    if (tool) return tool;
  }
  throw new Error(`No tool ${instance.tool} in ${instance.extension}`);
}
