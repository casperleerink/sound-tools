import type * as React from "react";

/** Accent names a tool can pick. One per tool type so instances are telling apart at a glance. */
export type Accent =
  | "teal"
  | "pink"
  | "yellow"
  | "blue"
  | "flamingo"
  | "green"
  | "orange"
  | "red"
  | "sky"
  | "maroon"
  | "rosewater";

export const ACCENT_VAR: Record<Accent, string> = {
  teal: "var(--color-teal-500)",
  pink: "var(--color-pink-500)",
  yellow: "var(--color-yellow-500)",
  blue: "var(--color-blue-500)",
  sky: "var(--color-cyan-500)",
  flamingo: "var(--color-flamingo-500)",
  green: "var(--color-green-500)",
  orange: "var(--color-orange-500)",
  red: "var(--color-red-500)",
  maroon: "var(--color-maroon-500)",
  rosewater: "var(--color-rosewater-500)",
};

export type PortKind = "audio" | "event" | "modulation";

export interface Port {
  id: string;
  label: string;
  kind: PortKind;
  direction: "in" | "out";
}

export interface ToolInstance {
  id: string;
  /** Composer-facing name, e.g. "Tone A". */
  name: string;
  /** Tool id inside the extension, e.g. "tone". */
  tool: string;
  /** Extension package name, e.g. "rhythm-loops". */
  extension: string;
  /** Path of the saved record inside the project. */
  record: string;
}

export interface ToolViewProps {
  instance: ToolInstance;
  /** Which of the tool's views this is. */
  viewId: string;
}

export interface ToolViewDef {
  id: string;
  label: string;
  component: React.ComponentType<ToolViewProps>;
}

export interface ToolDef {
  id: string;
  /** Tool type name shown in tabs, e.g. "Tone". */
  name: string;
  kind: "instrument" | "effect" | "sequencer" | "utility";
  accent: Accent;
  ports: Port[];
  views: ToolViewDef[];
}

export interface ExtensionDef {
  /** Package name, e.g. "rhythm-loops". */
  name: string;
  /** Human title, e.g. "Rhythm Loops". */
  title: string;
  description: string;
  tools: ToolDef[];
}
