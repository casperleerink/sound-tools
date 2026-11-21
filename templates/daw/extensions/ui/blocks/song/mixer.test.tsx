import { expect, test } from "bun:test";
import { act, apply, find, press, render, type Node, type Self } from "../../../sdk";
import { host } from "../../../sdk/view";
import { editsOf } from "../../../song/edits";
import { fakeSong, trackOf } from "../../../song/fake";
import { song } from "../../../song";
import { MIXER, pageOf } from "../../../song/view";
import { Mixer } from "./mixer";
import { SHORTEST_FADER, UNDER_FADER } from "../../strip";
import { travelled, VOLUME } from "../../value";

type Song = Self<typeof song>;
type Gain = number | "-inf";

const fader = (gain_db: Gain = 0, pan = 0, mute = false) => ({ gain_db, pan, mute });
const track = (name: string, title: string, mix = fader(), sends: Record<string, Gain> = {}, tool = "track") => ({
  name,
  tool,
  state: { title, colour: "blue", automation: [] },
  children: [
    { name: "mix", tool: "fader", state: mix, children: [] },
    ...Object.entries(sends).map(([back, gain_db]) => ({ name: `send-${back}`, tool: "fader", state: fader(gain_db), children: [] })),
  ],
});
const back = (name: string, title: string) => ({ name, tool: "bus", state: { title, colour: "teal", pre_fader: false }, children: [{ name: "mix", tool: "fader", state: fader(), children: [] }] });
const master = { name: "master", tool: "fader", state: fader(), children: [] };
const songOf = (children: { name: string; tool: string }[], order = children.flatMap((each) => (each.tool === "bus" ? [] : [each.name]))) =>
  ({ name: "song", state: { order, solo: [], sidechains: [], limit_off: false }, children: [...children, master] }) as unknown as Song;

/** The song with its mixer shown. */
const mixer = (self: Song, held: string[] = []) => {
  render(song, self, { held });
  if (!find("mixer")) act("song.mixer");
  return self;
};
const keysIn = (node: Node | undefined): string[] =>
  !node ? [] : [...("key" in node && node.key ? [node.key] : []), ...("children" in node && Array.isArray(node.children) ? node.children.flatMap(keysIn) : [])];
/** Whether `key` shows only under the pointer: inside a part with `reveal`. */
const quiet = (key: string, within: Node | undefined): boolean =>
  !!within && "children" in within && Array.isArray(within.children) && within.children.some((kid) => ("reveal" in kid && kid.reveal && keysIn(kid).includes(key)) || quiet(key, kid));
const step = (ops: ReturnType<typeof act>) => ops.map(([path, value]) => [path.join("."), value]);

test("M shows the mixer under the tracks in place of a panel, and its close hides it", () => {
  render(song, songOf([track("t00", "Drums")]));
  expect(find("mixer")).toBeUndefined();
  act("song.mixer");
  expect(find("mixer")).toBeDefined();
  expect(find("close-mixer")).toBeDefined();
  press("close-mixer");
  expect(find("mixer")).toBeUndefined();
});

test("a strip per track in their order in the part that scrolls; the returns and the master pinned after it, in view", () => {
  mixer(songOf([track("t01", "Bass"), track("t00", "Drums"), back("ret-a", "Reverb")], ["t01", "t00"]));
  const scrolled = find("mixer-tracks");
  // Placed in the plane in their order, a strip's width apart.
  const placed = (scrolled as { children: { place: number[]; children: { key: string }[] }[] }).children.map((each) => [each.children[0]!.key, each.place[0]]);
  expect(placed).toEqual([["mixer-t01", 4], ["mixer-t00", 96]]);
  expect([find("strip-name-t00", scrolled), find("strip-name-ret-a", scrolled), find("strip-name-master", scrolled)]).toMatchObject([{}, undefined, undefined]);
  expect([find("strip-name-ret-a"), find("strip-name-master")]).toMatchObject([{ label: "Choose Reverb strip" }, { label: "Choose Master strip" }]);
  expect(find("mixer")).toMatchObject({ h: "full", scroll: "y" });
});

test("a click on a strip's name chooses its track and keeps the mixer; a double click shows its panel", () => {
  mixer(songOf([track("t00", "Drums"), track("t01", "Bass")]));
  press("strip-name-t01");
  expect([find("strip-name-t01"), find("mixer")]).toMatchObject([{ value: true }, {}]);
  expect(find("strip-name-t00")).toMatchObject({ value: false });
  press("strip-name-t00", { clicks: 2 });
  expect(find("mixer")).toBeUndefined();
  expect(find("strip-t00")).toBeDefined();
});

test("the fader is on the fader's scale: 0 dB well up, a drag to its foot silences it as one step, a double click puts 0 dB back", () => {
  expect(travelled(0, VOLUME)).toBe(0.8);
  expect(travelled(6, VOLUME)).toBeGreaterThan(0.999);
  const self = mixer(songOf([track("t00", "Drums", fader(-6))]));
  expect(find("Volume Drums")).toMatchObject({ role: "slider", value: -6 });
  const silenced = act("fader-t00", "drag", { dy: 10_000 });
  expect(step(silenced)).toEqual([["children.0.children.0.state.gain_db", "-inf"]]);
  mixer(apply(self, silenced));
  expect(find("fader-t00")).toMatchObject({ value: "-inf" });
  expect(step(act("fader-t00", "reset"))).toEqual([["children.0.children.0.state.gain_db", 0]]);
});

test("an arrow up from silence steps off the bottom; typed -inf silences, typed 3 dB sets it", () => {
  mixer(songOf([track("t00", "Drums", fader("-inf"))]));
  const [, up] = act("fader-t00", "increment", 1)[0]!;
  expect(up).toBeLessThan(-60);
  expect(up).toBeGreaterThan(-120);
  mixer(songOf([track("t00", "Drums", fader(-6))]));
  // What it shows changes nothing.
  expect(act("Volume Drums value", "commit", "−6 dB")).toEqual([]);
  expect(step(act("Volume Drums value", "commit", "3"))).toEqual([["children.0.children.0.state.gain_db", 3]]);
  expect(step(act("Volume Drums value", "commit", "-inf"))).toEqual([["children.0.children.0.state.gain_db", "-inf"]]);
});

test("a strip says its volume and pan only away from where they rest, else under the pointer", () => {
  mixer(songOf([track("t00", "Drums"), track("t01", "Bass", fader(-4.5, -0.3))]));
  expect(quiet("fader-t00-value", find("strip-t00"))).toBe(true);
  expect(quiet("pan-t00-value", find("strip-t00"))).toBe(true);
  expect(quiet("fader-t01-value", find("strip-t01"))).toBe(false);
  expect(quiet("pan-t01-value", find("strip-t01"))).toBe(false);
});

test("pan is a dial typed at its number; mute and solo are switches, a return has no solo", () => {
  mixer(songOf([track("t00", "Drums"), back("ret-a", "Reverb")]));
  expect(find("Pan Drums")).toMatchObject({ role: "slider", value: 0 });
  expect(step(act("Pan Drums value", "commit", "25L"))).toEqual([["children.0.children.0.state.pan", -0.25]]);
  expect(step(act("strip-mute-t00", "toggle"))).toEqual([["children.0.children.0.state.mute", true]]);
  expect(step(act("strip-solo-t00", "toggle"))).toEqual([["state.solo", ["t00"]]]);
  expect([find("strip-mute-ret-a"), find("strip-solo-ret-a"), find("strip-mute-master")]).toMatchObject([{ role: "switch" }, undefined, { role: "switch" }]);
});

test("only an audio track's strip arms", () => {
  mixer(songOf([track("t00", "Drums"), track("audio-1", "Voice", fader(), {}, "tape")]));
  expect([find("strip-arm-t00"), find("strip-arm-audio-1")]).toMatchObject([undefined, { label: "Arm Voice" }]);
});

test("a row of send dials per return a track sends to, lined up across the strips beside the returns' names", () => {
  mixer(songOf([track("t00", "Drums", fader(), { "ret-a": -12, "ret-b": "-inf" }), track("t01", "Bass", fader(), { "ret-a": -6 }), back("ret-a", "Reverb"), back("ret-b", "Delay"), back("ret-c", "Chorus")]));
  expect(find("Reverb send of Drums")).toMatchObject({ role: "slider", value: -12 });
  expect(find("Delay send of Drums")).toMatchObject({ value: "-inf" });
  // Bass sends nothing to Delay: its row is empty, so the next lines up; nobody sends to Chorus: no row.
  expect(find("Delay send of Bass")).toBeUndefined();
  expect(find("Chorus send of Drums")).toBeUndefined();
  // A silent send is at rest: its number shows under the pointer.
  expect(quiet("send-t00-ret-b-value", find("Delay send of Drums"))).toBe(true);
  expect(step(act("Reverb send of Bass", "set", -3))).toEqual([["children.1.children.1.state.gain_db", -3]]);
  expect(step(act("Delay send of Drums value", "commit", "-10"))).toEqual([["children.0.children.2.state.gain_db", -10]]);
});

test("the faders take the height the sends leave, never shorter than the column beside them", () => {
  const sends = (count: number) => Object.fromEntries([..."abcdefgh"].slice(0, count).map((letter) => [`ret-${letter}`, -6]));
  const height = (count: number) => {
    mixer(songOf([track("t00", "Drums", fader(), sends(count)), ...[..."abcdefgh"].slice(0, count).map((letter) => back(`ret-${letter}`, letter))]));
    return (find("strip-t00") as { h: number }).h - UNDER_FADER;
  };
  const [none, two, eight] = [height(0), height(2), height(8)];
  expect(none).toBeLessThan(MIXER);
  expect(two).toBeLessThan(none);
  expect(eight).toBe(SHORTEST_FADER);
});

test("a fader a lane plays shows what plays and takes no change", () => {
  mixer(songOf([track("t00", "Drums")]), ["t00/mix.gain_db"]);
  expect(find("fader-t00")).toMatchObject({ value: { node: { op: "value" } } });
  expect(() => act("fader-t00", "set", -6)).toThrow();
});

test("a strip is one stop of Tab, whose controls the arrows walk", () => {
  mixer(songOf([track("t00", "Drums")]));
  expect(find("strip-t00")).toMatchObject({ group: true, role: "group" });
});

test("a fader moved builds that strip again and no other: the window keeps the rest", () => {
  const f = fakeSong({ children: [trackOf("t00"), trackOf("t01"), trackOf("t02"), master] });
  const held = (_: string) => false;
  const ctx = Object.assign(f.song, { held, shortcut: () => null });
  const draw = (self: unknown) => Mixer({ page: pageOf(self as never, ctx) });
  const run = () => host.run("mixer-test", draw as never, { name: "song", state: f.self.state, children: f.self.children.map((each) => ({ ...(each as object), facts: { title: "", fields: [] } })), facts: { title: "", fields: [] } } as never, ctx as never);
  expect(Object.keys(run().memos)).toEqual(["mixer-t00", "mixer-t01", "mixer-t02", "mixer-master"]);
  expect(Object.keys(run().memos)).toEqual([]);
  editsOf(f.song).tracks.volume("t01", -6);
  expect(Object.keys(run().memos)).toEqual(["mixer-t01"]);
});
