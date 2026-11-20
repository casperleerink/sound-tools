import { expect, test } from "bun:test";
import { act, find, h, press, render, type Offer } from "../../../sdk";
import { regionOf, tapeOf, trackOf } from "../../../song/fake";
import { memory as fresh, type Song } from "../../../song/model";
import { pageOf } from "../../../song/view";
import { Rack } from "./rack";

const offer = (key: string, name: string, group: string, takes: Offer["takes"], unavailable: string | null = null): Offer =>
  ({ key, name, detail: null, group, icon: "device-plugin", takes, unavailable });
const CATALOG = [
  offer("instrument.synth", "Synth", "Built-in", "notes"), offer("wavetable", "Wavetable", "Built-in", "notes"),
  offer("eq", "EQ", "Tone", "audio"), offer("compressor", "Compressor", "Dynamics", "audio"), offer("delay", "Delay", "Space", "audio"),
  offer("vst3:far", "Far", "Plugins", "audio", "This plugin did not load."),
];
const device = (name: string, tool: string) => ({ name, tool, state: {}, children: [] });
const chainOf = (order: string[], off: string[] = [], tools: Record<string, string> = {}) =>
  ({ name: "chain", tool: "chain", state: { order, off }, children: order.map((name) => device(name, tools[name] ?? name)) });
const fader = (name: string, gain_db: number | "-inf") => ({ name, tool: "fader", state: { gain_db, pan: 0, mute: false }, children: [] });
const returnOf = (name: string, title: string, effects: string[] = []) =>
  ({ name, tool: "bus", state: { title, colour: "mauve", pre_fader: false }, children: [chainOf(effects), fader("mix", 0)] });
const song = (children: unknown[], state: object = {}) =>
  ({ name: "song", state: { order: [], solo: [], sidechains: [], limit_off: false, ...state }, children: [...children, fader("master", 0), device("limit", "limiter")] });
/** Track 1: the synth, then a compressor, an EQ that is off and a delay, sending into Return A. */
const rich = (state: object = {}) =>
  song([trackOf("t00", [device("synth", "instrument.synth"), chainOf(["comp", "eq", "dly"], ["eq"], { comp: "compressor", dly: "delay" }), fader("send-ret-a", -12)]), trackOf("t01"), returnOf("ret-a", "Return A")], state);

/** The rack of `name` as the song draws it, with a catalog, and what it put with `addOffer`. */
function rack(self: ReturnType<typeof song>, name = "t00", setup: (memory: ReturnType<typeof fresh>) => void = () => {}) {
  const memory = fresh();
  memory.below = { kind: "track", name };
  setup(memory);
  const offers: string[][] = [];
  let shown: unknown = self;
  const view = (drawn: Parameters<typeof pageOf>[0], ctx: Song) => {
    shown = drawn;
    const song = { ...ctx, memory, catalog: CATALOG, addOffer: (...said: string[]) => offers.push(said), shortcut: (action: string) => ({ effect_earlier: "alt-left", effect_later: "alt-right" })[action] ?? null } as unknown as Song;
    return <Rack page={pageOf(drawn, song)} name={name} />;
  };
  render(view, self as never);
  return { memory, offers, get self() { return shown; } };
}
type Part = { name: string; state: { order: string[]; off: string[] }; children: Part[] };
/** The chain of the track or return `name` as it is now. */
const chain = (self: unknown, name = "t00") =>
  (self as Part).children.find((child) => child.name === name)!.children.find((child) => child.name === "chain")!.state;

test("a track shows its instrument, then its effects in order, each in a frame with its title", () => {
  rack(rich());
  expect(["slot-instrument", "slot-comp", "slot-eq", "slot-dly"].map((key) => find(key)).map((slot) => slot && "label" in slot && slot.label)).toEqual(["Device Synth", "Device Compressor", "Device EQ", "Device Delay"]);
  expect(find("slot-instrument-power")).toBeUndefined();
  expect(find("Turn EQ on")).toMatchObject({ checked: false });
});

test("power turns an effect off and on, and remove takes it out with its sidechain, each one step", () => {
  const shown = rack(rich({ sidechains: [{ effect: "t00/chain/comp", from: "t01", tap: "post_fx" }] }));
  press("Turn Compressor off");
  expect(chain(shown.self).off).toEqual(["eq", "comp"]);
  press("Turn EQ on");
  expect(chain(shown.self).off).toEqual(["comp"]);
  press("Remove Compressor");
  expect([chain(shown.self), (shown.self as { state: object }).state]).toMatchObject([{ order: ["eq", "dly"], off: [] }, { sidechains: [] }]);
});

test("Add effect lists what takes sound, grouped, with what cannot be added greyed; a pick joins the chain last, chosen", () => {
  const shown = rack(rich());
  const { memory, offers } = shown;
  expect(find("add-effect-wavetable")).toBeUndefined();
  expect(find("add-effect-vst3:far")).toMatchObject({ label: "Far" });
  expect(() => press("add-effect-vst3:far")).toThrow();
  press("add-effect-compressor");
  expect(offers).toEqual([["compressor", "t00/chain/compressor", "Add an effect"]]);
  expect(chain(shown.self).order).toEqual(["comp", "eq", "dly", "compressor"]);
  expect(memory.effect).toEqual({ track: "t00", name: "compressor" });
});

test("an effect's title menu moves it a step, with its keys, and not past the ends", () => {
  const shown = rack(rich());
  const { memory } = shown;
  expect(find("slot-comp-title-earlier")).toMatchObject({ label: "Move earlier" });
  expect(() => press("slot-comp-title-earlier")).toThrow();
  press("slot-comp-title-later");
  expect(chain(shown.self).order).toEqual(["eq", "comp", "dly"]);
  expect(memory.effect).toEqual({ track: "t00", name: "comp" });
  expect(() => press("slot-dly-title-later")).toThrow();
});

test("a pick in an effect's title menu replaces it where it is and lets go of its sidechain; the instrument's title replaces the instrument", () => {
  const { offers } = rack(rich({ sidechains: [{ effect: "t00/chain/comp", from: "t01", tap: "post_fx" }] }));
  expect(find("slot-comp-title-compressor")).toMatchObject({ checked: true });
  expect(press("slot-comp-title-delay")).toEqual([[["state", "sidechains"], []]]);
  press("slot-instrument-title-wavetable");
  expect(offers).toEqual([["delay", "t00/chain/comp", "Change an effect"], ["wavetable", "t00/synth", "Change the instrument"]]);
  expect(find("slot-instrument-title-instrument.synth")).toMatchObject({ checked: true });
});

test("an effect dragged onto another goes before it; onto the instrument, first; onto Add effect, last", () => {
  const shown = rack(rich());
  act("slot-dly-header", "drag", { dx: -400, over: "slot-comp-title" });
  expect(chain(shown.self).order).toEqual(["dly", "comp", "eq"]);
  act("slot-comp-header", "drag", { dx: 400, over: "add-effect" });
  expect(chain(shown.self).order).toEqual(["dly", "eq", "comp"]);
  act("slot-comp-header", "drag", { dx: -400, over: "slot-instrument" });
  expect(chain(shown.self).order).toEqual(["comp", "dly", "eq"]);
  expect(act("slot-dly-header", "drag", { dx: 40, over: "slot-dly" })).toEqual([]);
});

test("a press on an effect's header chooses it, which its frame shows", () => {
  const { memory } = rack(rich());
  press("slot-eq-header");
  expect(memory.effect).toEqual({ track: "t00", name: "eq" });
  expect(find("slot-eq")).toMatchObject({ border: "text-muted" });
  expect(find("slot-comp")).toMatchObject({ border: "border-strong" });
});

test("an effect that takes a sidechain picks the track that keys it, and where on another track", () => {
  rack(rich());
  expect(find("Sidechain of EQ")).toBeUndefined();
  expect(act("sidechain-comp-t01", "press")).toEqual([[["state", "sidechains"], [{ effect: "t00/chain/comp", from: "t01", tap: "post_fx" }]]]);
  expect(find("Sidechain tap of Compressor")).toBeDefined();
  expect(act("tap-comp-post_fader", "press")).toEqual([[["state", "sidechains", 0, "tap"], "post_fader"]]);
  act("sidechain-comp-t00", "press");
  expect(find("Sidechain tap of Compressor")).toBeUndefined();
});

test("a track's panel ends with a send for each return, typed in dB", () => {
  rack(rich());
  expect(find("Send to Return A")).toMatchObject({ value: -12 });
  expect(act("Send to Return A", "set", -6)).toEqual([[["children", 0, "children", 3, "state", "gain_db"], -6]]);
});

test("a return's panel takes effects and Pre fader, and with none says so", () => {
  rack(rich(), "ret-a");
  expect(find("No devices yet")).toBeUndefined();
  expect(find("add-effect")).toBeDefined();
  expect(press("Pre fader Return A")).toEqual([[["children", 2, "state", "pre_fader"], true]]);
  expect(find("Send to Return A")).toBeUndefined();
});

test("the master's panel is its strip and its limiter, which power turns off", () => {
  rack(rich(), "master");
  expect(find("slot-limit")).toMatchObject({ label: "Device Limiter" });
  expect(find("add-effect")).toBeUndefined();
  expect(press("Turn Limiter off")).toEqual([[["state", "limit_off"], true]]);
});

test("an audio track shows its region chosen alone, else asks for one", () => {
  const self = song([tapeOf("audio-1", [chainOf([]), regionOf("r1", 0, 4), regionOf("r2", 0)])]);
  rack(self, "audio-1");
  expect(find("clip-empty")).toMatchObject({ label: "Device Clip" });
  rack(self, "audio-1", (memory) => (memory.selected = ["audio-1/r1"]));
  expect(find("slot-region")).toMatchObject({ label: "Device r1.wav" });
  expect(act("Gain of r1.wav", "set", -6)).toEqual([[["children", 0, "children", 2, "state", "gain_db"], -6]]);
  rack(self, "audio-1", (memory) => (memory.selected = ["audio-1/r1", "audio-1/r2"]));
  expect(find("slot-region")).toBeUndefined();
});

test("the panel's strip names its pan and says the volume and pan at rest, with the input of an audio track over mute and solo", () => {
  rack(rich());
  const strip = JSON.stringify(find("strip-t00"));
  expect(strip).toContain('"text":"Pan"');
  expect(strip).not.toContain('"reveal":true');
  rack(song([tapeOf("audio-1", [chainOf([])])]), "audio-1");
  expect(find("input-audio-1", find("strip-audio-1"))).toMatchObject({ label: "Input of Audio 1", w: 64 });
});

test("an audio clip's card draws its wave from the clip's own sound", () => {
  const self = song([tapeOf("audio-1", [chainOf([]), regionOf("r1", 0, 4)])]);
  rack(self, "audio-1", (memory) => (memory.selected = ["audio-1/r1"]));
  const card = JSON.stringify(find("slot-region"));
  expect(card).toContain('{"op":"feed","name":"wave","path":"audio-1/r1"}');
});
