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
  devices: ["MacBook Pro Speakers", "Scarlett 2i2", "AirPods Pro"],
};

export const instances: Instance[] = [
  { id: "poly-1", type: "polyrhythm", name: "Three voices" },
  { id: "add-1", type: "additive", name: "Drone" },
  { id: "mix-1", type: "mixer", name: "Mix" },
];

/* Signal flows left to right: patterns, instrument, mix. Cards sit at least 48px apart. */
export const initialCards: CardPlacement[] = [
  { id: "c1", instanceId: "poly-1", viewId: "patterns", x: 0, y: 0 },
  { id: "c2", instanceId: "add-1", viewId: "partials", x: 504, y: 56 },
  { id: "c3", instanceId: "mix-1", viewId: "strips", x: 920, y: 0 },
];

export const connections: Connection[] = [
  { id: "k1", from: { instanceId: "poly-1", portId: "voice-a" }, to: { instanceId: "add-1", portId: "gate" }, kind: "event" },
  { id: "k2", from: { instanceId: "add-1", portId: "out" }, to: { instanceId: "mix-1", portId: "in-1" }, kind: "audio" },
];

export function getInstance(id: string): Instance {
  const inst = instances.find((i) => i.id === id);
  if (!inst) throw new Error(`Unknown instance "${id}"`);
  return inst;
}
