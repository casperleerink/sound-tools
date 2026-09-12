/* Fake project state: instances, open cards on the canvas, and connections. */
import type { PortKind } from "./sdk";

export interface Instance {
  id: string;
  type: string;
  name: string;
}

export interface CardPlacement {
  id: string;
  instanceId: string;
  viewId: string;
  x: number;
  y: number;
  /** Only one card per instance carries the ports (and therefore the connections). */
  primary: boolean;
}

export interface Connection {
  id: string;
  from: { instanceId: string; portId: string };
  to: { instanceId: string; portId: string };
  kind: PortKind;
}

export const project = {
  name: "Tidal Studies",
  folder: "~/Music/Tidal Studies",
  device: "MacBook Pro Speakers",
  sampleRate: 48_000,
};

export const instances: Instance[] = [
  { id: "poly-1", type: "polyrhythm", name: "Three voices" },
  { id: "curve-1", type: "curve", name: "Brightness sweep" },
  { id: "smp-1", type: "sampler", name: "Field recording" },
  { id: "add-1", type: "additive", name: "Drone" },
  { id: "mix-1", type: "mixer", name: "Mix" },
];

/* Three columns, signal flows left to right: sources, instruments, mix.
   Column x: 32 / 520 / 948. Widths come from each view definition. The canvas
   fits the whole scene into the window on load (90% at 1440x900). */
const COL = [32, 520, 948] as const;

export const initialCards: CardPlacement[] = [
  { id: "c1", instanceId: "poly-1", viewId: "patterns", x: COL[0], y: 72, primary: true },
  { id: "c2", instanceId: "poly-1", viewId: "voices", x: COL[0], y: 344, primary: false },
  { id: "c3", instanceId: "curve-1", viewId: "draw", x: COL[0], y: 582, primary: true },
  { id: "c4", instanceId: "smp-1", viewId: "wave", x: COL[1], y: 72, primary: true },
  { id: "c5", instanceId: "add-1", viewId: "partials", x: COL[1], y: 390, primary: true },
  { id: "c6", instanceId: "mix-1", viewId: "strips", x: COL[2], y: 72, primary: true },
  { id: "c7", instanceId: "mix-1", viewId: "sends", x: COL[2], y: 408, primary: false },
];

export const connections: Connection[] = [
  { id: "k1", from: { instanceId: "poly-1", portId: "voice-a" }, to: { instanceId: "smp-1", portId: "trigger" }, kind: "event" },
  { id: "k2", from: { instanceId: "poly-1", portId: "voice-b" }, to: { instanceId: "add-1", portId: "gate" }, kind: "event" },
  { id: "k3", from: { instanceId: "poly-1", portId: "voice-c" }, to: { instanceId: "add-1", portId: "gate" }, kind: "event" },
  { id: "k4", from: { instanceId: "curve-1", portId: "out" }, to: { instanceId: "add-1", portId: "brightness" }, kind: "mod" },
  { id: "k5", from: { instanceId: "smp-1", portId: "out" }, to: { instanceId: "mix-1", portId: "in-1" }, kind: "audio" },
  { id: "k6", from: { instanceId: "add-1", portId: "out" }, to: { instanceId: "mix-1", portId: "in-2" }, kind: "audio" },
];

export function getInstance(id: string): Instance {
  const inst = instances.find((i) => i.id === id);
  if (!inst) throw new Error(`Unknown instance "${id}"`);
  return inst;
}
