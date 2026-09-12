/* The (fake) UI SDK contract an extension implements. Extensions export one
   ToolDefinition; the core places its views on the canvas and draws its ports. */
import type * as React from "react";
import type { Accent } from "@/lib/accent";

export type PortKind = "audio" | "event" | "mod";

export interface PortDef {
  id: string;
  label: string;
  kind: PortKind;
}

export interface ToolViewProps {
  instanceId: string;
  instanceName: string;
  accent: Accent;
}

export interface ViewDef {
  id: string;
  label: string;
  /** Card width on the canvas in px. Height follows content. */
  width: number;
  component: React.ComponentType<ToolViewProps>;
}

export interface ToolDefinition {
  /** Stable type id, e.g. "polyrhythm". */
  type: string;
  /** Human name of the tool type, e.g. "Polyrhythm". */
  name: string;
  description: string;
  accent: Accent;
  inputs: PortDef[];
  outputs: PortDef[];
  /** First view is the default one. */
  views: ViewDef[];
}
