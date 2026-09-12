import type { ToolInstance } from "@/sdk/types";

export const PROJECT = {
  name: "Three voices",
  folder: "~/Music/Sound Tools/three-voices",
  device: { name: "MacBook Pro Speakers", sampleRate: 48000, bufferSize: 256 },
};

export const INSTANCES: ToolInstance[] = [
  { id: "loops", name: "Loops", tool: "loops", extension: "rhythm-loops", record: "state/loops.json" },
  { id: "tone-a", name: "Tone A", tool: "tone", extension: "tone", record: "state/tone-a.json" },
  { id: "tone-b", name: "Tone B", tool: "tone", extension: "tone", record: "state/tone-b.json" },
  { id: "tone-c", name: "Tone C", tool: "tone", extension: "tone", record: "state/tone-c.json" },
  { id: "tremolo", name: "Tremolo", tool: "tremolo", extension: "tremolo", record: "state/tremolo.json" },
  { id: "lattice", name: "Lattice", tool: "lattice", extension: "ratio-lattice", record: "state/lattice.json" },
  { id: "mix", name: "Mix", tool: "mixer", extension: "mixer", record: "state/mix.json" },
];

export interface Connection {
  from: { instance: string; port: string };
  to: { instance: string; port: string };
  kind: "audio" | "event" | "modulation";
}

export const CONNECTIONS: Connection[] = [
  { from: { instance: "loops", port: "voice-1" }, to: { instance: "tone-a", port: "trigger" }, kind: "event" },
  { from: { instance: "loops", port: "voice-2" }, to: { instance: "tone-b", port: "trigger" }, kind: "event" },
  { from: { instance: "loops", port: "voice-3" }, to: { instance: "tone-c", port: "trigger" }, kind: "event" },
  { from: { instance: "lattice", port: "pitch" }, to: { instance: "tone-a", port: "pitch" }, kind: "modulation" },
  { from: { instance: "tone-a", port: "out" }, to: { instance: "tremolo", port: "in" }, kind: "audio" },
  { from: { instance: "tremolo", port: "out" }, to: { instance: "mix", port: "ch-1" }, kind: "audio" },
  { from: { instance: "tone-b", port: "out" }, to: { instance: "mix", port: "ch-2" }, kind: "audio" },
  { from: { instance: "tone-c", port: "out" }, to: { instance: "mix", port: "ch-3" }, kind: "audio" },
];

export function instanceById(id: string): ToolInstance {
  const found = INSTANCES.find((i) => i.id === id);
  if (!found) throw new Error(`Unknown instance ${id}`);
  return found;
}

export function connectionsFor(instanceId: string) {
  return {
    inputs: CONNECTIONS.filter((c) => c.to.instance === instanceId),
    outputs: CONNECTIONS.filter((c) => c.from.instance === instanceId),
  };
}
