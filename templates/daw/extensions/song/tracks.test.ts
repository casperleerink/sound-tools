import { expect, test } from "bun:test";
import { editsOf } from "./edits";
import { clipOf, fakeSong, laneOf, regionOf, tapeOf, trackOf } from "./fake";
import { BAR, BEAT } from "./time";

type Fake = ReturnType<typeof fakeSong>;
const named = (f: Fake, name: string) => (f.self.children as any[]).find((each) => each.name === name);
const names = (f: Fake, name: string) => named(f, name).children.map((child: { name: string }) => child.name);
const back = (name: string) => ({ name, tool: "bus", state: { title: "", colour: "teal", pre_fader: false }, children: [{ name: "chain", tool: "chain", state: { order: [], off: [] }, children: [] }] });

test("a track added has an instrument, effects, a fader, a recorder and a silent send into every return, in the next colour", () => {
  const f = fakeSong({ state: { order: ["t00"] }, children: [trackOf("t00"), back("ret-a")] });
  editsOf(f.song).tracks.add("instrument");
  const added = named(f, "track");
  expect(added.state).toMatchObject({ title: "Track 2", colour: "peach" });
  expect(names(f, "track")).toEqual(["synth", "chain", "mix", "rec", "send-ret-a"]);
  expect(added.children[4].state).toEqual({ gain_db: "-inf" });
  expect(f.self.state.order).toEqual(["t00", "track"]);
  expect(f.steps).toEqual(["Add a track"]);
});

test("three returns added have three names, each a silent send from every track, and the returns open", () => {
  const f = fakeSong({ children: [trackOf("t00"), tapeOf("audio-1")] });
  const { tracks } = editsOf(f.song);
  for (let times = 0; times < 3; times++) tracks.add("return");
  expect(["ret-a", "ret-b", "ret-c"].map((name) => named(f, name).state.title)).toEqual(["Return 1", "Return 2", "Return 3"]);
  expect(names(f, "audio-1").filter((name: string) => name.startsWith("send-"))).toEqual(["send-ret-a", "send-ret-b", "send-ret-c"]);
  expect(f.memory.returnsOpen).toBe(true);
});

test("an audio track added records the first input; the next is audio-2", () => {
  const f = fakeSong({ children: [] });
  editsOf(f.song).tracks.add("audio");
  expect(named(f, "audio-1").state.title).toBe("Audio 1");
  expect(named(f, "audio-1").children[0]).toEqual({ name: "rec", tool: "recorder", state: { channels: [0] } });
  expect(f.steps).toEqual(["Add an audio track"]);
  editsOf(f.song).tracks.add("audio");
  expect(named(f, "audio-2").state.title).toBe("Audio 2");
});

test("a track is coloured, and deleted with the sends into it, its solo, its sidechains and what chose it", () => {
  const f = fakeSong({ state: { order: ["t00", "ret-a"], solo: ["ret-a"], sidechains: [{ effect: "t00/chain/comp", from: "ret-a", tap: "post_fader" }] }, children: [trackOf("t00", [{ name: "send-ret-a", tool: "fader", state: { gain_db: 0 } }]), back("ret-a")] });
  const { tracks } = editsOf(f.song);
  tracks.colour("t00", "sapphire");
  expect(named(f, "t00").state.colour).toBe("sapphire");
  f.memory.below = { kind: "track", name: "ret-a" };
  tracks.remove("ret-a");
  expect(names(f, "t00")).toEqual(["mix"]);
  expect(f.self.state).toMatchObject({ order: ["t00"], solo: [], sidechains: [] });
  expect(f.memory.below).toBe(null);
  expect(f.steps).toEqual(["Colour a track", "Delete a track"]);
});

test("a return deleted takes the lanes of the sends into it with it", () => {
  const send = { name: "send-ret-a", tool: "fader", state: { gain_db: 0 } };
  const f = fakeSong({ state: { order: ["t00"] }, children: [trackOf("t00", [send, laneOf("lane-send", [{ tick: 0, value: 0 }])], [{ lane: "lane-send", target: "send-ret-a.gain_db" }]), back("ret-a")] });
  editsOf(f.song).tracks.remove("ret-a");
  expect([names(f, "t00"), named(f, "t00").state.automation]).toEqual([["mix"], []]);
});

test("delete asks first, and Cancel keeps the track", async () => {
  const kept = fakeSong({ children: [trackOf("t00")] }, { answers: { confirm: false } });
  await editsOf(kept.song).tracks.askToRemove("t00");
  expect(named(kept, "t00")).toBeDefined();
  const gone = fakeSong({ children: [trackOf("t00")] }, { answers: { confirm: true } });
  await editsOf(gone.song).tracks.askToRemove("t00");
  expect(named(gone, "t00")).toBeUndefined();
});

test("a head dragged onto another goes before it; the keys choose the track above and move it", () => {
  const f = fakeSong({ state: { order: ["t00", "t01", "t02"] }, children: [trackOf("t00"), trackOf("t01"), trackOf("t02")] });
  const { tracks } = editsOf(f.song);
  tracks.moveBefore("t02", "t00");
  expect(f.self.state.order).toEqual(["t02", "t00", "t01"]);
  expect(tracks.step(-1, false)).toBe(false);
  tracks.openPanel("t01");
  tracks.step(-1, false);
  expect([f.memory.below, f.selected.at(-1)]).toEqual([{ kind: "track", name: "t00" }, "t00"]);
  tracks.step(1, true);
  expect(f.self.state.order).toEqual(["t02", "t01", "t00"]);
  expect(f.steps).toEqual(["Move a track", "Move a track"]);
});

test("mute and solo are one step each on the track they name", () => {
  const f = fakeSong({ children: [trackOf("t00"), trackOf("t01")] });
  const { tracks } = editsOf(f.song);
  tracks.mute("t01");
  tracks.solo("t00");
  tracks.solo("t00");
  expect(named(f, "t01").children[0].state.mute).toBe(true);
  expect(f.self.state.solo).toEqual([]);
  expect(f.steps).toEqual(["Mute a track", "Solo a track", "Solo a track"]);
});

test("arm opens the recorder's input, and R records every armed track and the keyboard on the chosen track", () => {
  const rec = { name: "rec", tool: "recorder", state: { channels: [] } };
  const f = fakeSong({ state: { order: ["t00", "audio-1"] }, children: [trackOf("t00", [rec]), tapeOf("audio-1", [rec])] });
  const { tracks } = editsOf(f.song);
  tracks.arm("audio-1");
  tracks.record();
  expect(f.sent).toEqual([
    [{ child: "audio-1/rec", number: "arm" }, 1],
    [{ child: "audio-1/rec", number: "record" }, 1],
    [{ child: "t00/rec", number: "record" }, 1],
  ]);
  expect(f.clock.asked).toBe(true);
  f.clock.playing = true;
  tracks.stopped();
  f.clock.playing = false;
  tracks.stopped();
  expect(f.memory.recording).toBe(null);
  expect(f.sent.slice(3)).toEqual([[{ child: "t00/rec", number: "record" }, 0], [{ child: "audio-1/rec", number: "record" }, 0]]);
});

test("R on a stopped song records on while the engine has not yet said it plays", () => {
  const rec = { name: "rec", tool: "recorder", state: { channels: [] } };
  const f = fakeSong({ state: { order: ["t00"] }, children: [trackOf("t00", [rec])] });
  const { tracks } = editsOf(f.song);
  tracks.record();
  tracks.stopped();
  tracks.stopped();
  expect(f.memory.recording).not.toBe(null);
  expect(f.sent).toEqual([[{ child: "t00/rec", number: "record" }, 1]]);
});

test("R with nothing armed and an audio track chosen arms it", () => {
  const rec = { name: "rec", tool: "recorder", state: { channels: [0] } };
  const f = fakeSong({ children: [tapeOf("audio-1", [rec])] });
  f.memory.track = "audio-1";
  editsOf(f.song).tracks.record();
  expect(f.memory.armed).toEqual(["audio-1"]);
  expect(f.sent).toEqual([[{ child: "audio-1/rec", number: "arm" }, 1], [{ child: "audio-1/rec", number: "record" }, 1]]);
});

test("a track renders onto an audio track under it that takes its fader, sends and their lanes, and mutes it", async () => {
  const f = fakeSong(
    { state: { order: ["t00", "t01"], solo: ["t00"] }, children: [trackOf("t00", [clipOf("c00", BAR, BAR), { name: "send-ret-a", tool: "fader", state: { gain_db: -6 } }, laneOf("lane-vol", [{ tick: 0, value: -3 }], "fader")], [{ lane: "lane-vol", target: "mix.gain_db" }]), trackOf("t01")] },
    { answers: { render: "Track 1.wav" } },
  );
  await editsOf(f.song).tracks.render("t00", ["t00/c00"], true);
  const audio = named(f, "audio-1");
  expect(audio.state).toMatchObject({ title: "Track 1 (audio)", automation: [{ lane: "lane-vol", target: "mix.gain_db" }] });
  expect(audio.children.map((child: { name: string }) => child.name)).toEqual(["rec", "chain", "mix", "send-ret-a", "lane-vol", "render"]);
  expect(audio.children.at(-1).state).toEqual({ asset: "Track 1.wav", start: BAR });
  expect(f.self.state).toMatchObject({ order: ["t00", "audio-1", "t01"], solo: ["t00", "audio-1"] });
  expect(named(f, "t00").children[0].state.mute).toBe(true);
  expect(f.steps).toEqual(["Render to audio track"]);
});

test("a send, the volume, pre fader and the limiter are each one step in plain words", () => {
  const f = fakeSong({ children: [trackOf("t00", [{ name: "send-ret-a", tool: "fader", state: { gain_db: "-inf" } }]), { ...back("ret-a"), state: { title: "Hall", colour: "teal", pre_fader: false } }, { name: "master", tool: "fader", state: { gain_db: 0 }, children: [] }] });
  const { tracks } = editsOf(f.song);
  tracks.send("t00", "ret-a", -12);
  tracks.volume("master", -3);
  tracks.preFader("ret-a");
  tracks.limiter();
  expect([named(f, "t00").children[1].state.gain_db, named(f, "master").state.gain_db, named(f, "ret-a").state.pre_fader, f.self.state.limit_off]).toEqual([-12, -3, true, true]);
  expect(f.steps).toEqual(["Change hall", "Change the volume", "Send before the faders", "Turn the limiter off"]);
});

test("the mixer and a panel take turns under the timeline", () => {
  const f = fakeSong({ children: [trackOf("t00")] });
  const { tracks } = editsOf(f.song);
  tracks.openPanel("t00");
  tracks.toggleMixer();
  expect(f.memory.below).toEqual({ kind: "mixer" });
  tracks.openPanel("t00");
  expect(f.memory.below).toEqual({ kind: "track", name: "t00" });
});

const chained = (order: string[], sidechains: { effect: string; from: string; tap: "pre_fx" | "post_fx" | "post_fader" }[] = []) =>
  fakeSong({ state: { sidechains }, children: [trackOf("t00", [{ name: "chain", tool: "chain", state: { order, off: [] }, children: order.map((name) => ({ name, tool: "eq", state: {}, children: [] })) }]), trackOf("t01")] });
const order = (f: Fake) => named(f, "t00").children[1].state;

test("an effect added joins the chain last, chosen; it moves a step, before another, or last", () => {
  const f = chained(["eq", "comp"]);
  const effects = editsOf(f.song).effects("t00");
  effects.add("reverb");
  expect(f.offers).toEqual([["reverb", "t00/chain/reverb", "Add an effect"]]);
  expect(order(f).order).toEqual(["eq", "comp", "reverb"]);
  expect(f.memory.effect).toEqual({ track: "t00", name: "reverb" });
  effects.step("reverb", -1);
  expect(order(f).order).toEqual(["eq", "reverb", "comp"]);
  effects.moveBefore("eq", null);
  expect(order(f).order).toEqual(["reverb", "comp", "eq"]);
  effects.step("eq", 1);
  expect(f.steps).toEqual(["Add an effect", "Move an effect", "Move an effect"]);
});

test("an effect turns off and on, and goes with its sidechain", () => {
  const f = chained(["comp"], [{ effect: "t00/chain/comp", from: "t01", tap: "post_fx" }]);
  const effects = editsOf(f.song).effects("t00");
  effects.toggle("comp");
  expect(order(f).off).toEqual(["comp"]);
  effects.remove("comp");
  expect([order(f).order, order(f).off, f.self.state.sidechains]).toEqual([[], [], []]);
  expect(f.steps).toEqual(["Turn an effect off", "Remove an effect"]);
});

test("a sidechain from its own track keys before its effects, from another after them, and Off takes it away", () => {
  const f = chained(["comp"]);
  const effects = editsOf(f.song).effects("t00");
  effects.sidechain("comp", "t00");
  expect(f.self.state.sidechains).toEqual([{ effect: "t00/chain/comp", from: "t00", tap: "pre_fx" }]);
  effects.sidechain("comp", null);
  effects.sidechain("comp", "t01");
  effects.tap("comp", "post_fader");
  expect(f.self.state.sidechains).toEqual([{ effect: "t00/chain/comp", from: "t01", tap: "post_fader" }]);
  expect(f.steps).toEqual(["Turn on sidechain", "Turn off sidechain", "Turn on sidechain", "Change sidechain tap"]);
});

/** Track 1 with `chain` holding an EQ and a compressor, a lane on the EQ, an LFO on the compressor and a lane on its volume. */
const automated = () => {
  const f = chained(["eq", "comp"]);
  const t00 = named(f, "t00");
  t00.children.push(laneOf("lane-eq", [{ tick: 0, value: 0 }]), { name: "lfo-comp", tool: "lfo", state: {}, children: [] }, laneOf("lane-vol", [{ tick: 0, value: 0 }]));
  t00.state.automation = [{ lane: "lane-eq", target: "chain/eq.bands[0].gain_db" }, { lane: "lfo-comp", target: "chain/comp.ratio" }, { lane: "lane-vol", target: "mix.gain_db" }];
  return f;
};

test("an effect removed or replaced takes the lanes of its numbers with it, in the same step", () => {
  const f = automated();
  const effects = editsOf(f.song).effects("t00");
  effects.remove("eq");
  expect(named(f, "t00").state.automation.map(({ lane }: { lane: string }) => lane)).toEqual(["lfo-comp", "lane-vol"]);
  expect(names(f, "t00")).toEqual(["mix", "chain", "lfo-comp", "lane-vol"]);
  effects.replace("delay", "comp");
  expect(names(f, "t00")).toEqual(["mix", "chain", "lane-vol"]);
  expect(f.steps).toEqual(["Remove an effect", "Change an effect"]);
});

test("an instrument picked takes the place of the synth", () => {
  const f = chained([]);
  editsOf(f.song).effects("t00").replace("wavetable");
  expect(f.offers).toEqual([["wavetable", "t00/synth", "Change the instrument"]]);
});

test("a region trims from its end, never past the end of its file", () => {
  const f = fakeSong({ children: [tapeOf("audio-1", [regionOf("whole", 0), regionOf("cut", 0, 1)])] });
  const { regions } = editsOf(f.song);
  regions.trimEnd("audio-1/whole", { dx: BEAT }, 2);
  expect(named(f, "audio-1").children[1].state.file_end_seconds).toBe(null);
  regions.trimEnd("audio-1/whole", { dx: -BEAT }, 2);
  expect(Math.round(named(f, "audio-1").children[1].state.file_end_seconds * 100) / 100).toBe(1.5);
  regions.trimEnd("audio-1/cut", { dx: BEAT }, 1);
  expect(Math.round(named(f, "audio-1").children[2].state.file_end_seconds * 100) / 100).toBe(1.5);
  expect(f.steps).toEqual(["Trim a region", "Trim a region"]);
});

test("a region's start trims what it plays of its file; its fades and gain are one step each", () => {
  const f = fakeSong({ children: [tapeOf("audio-1", [regionOf("r", BAR, 4)])] });
  const { regions, clips } = editsOf(f.song);
  regions.trimStart("audio-1/r", { dx: BEAT });
  regions.fadeIn("audio-1/r", { dx: BEAT }, 3.5);
  regions.fadeOut("audio-1/r", { dx: -100 * BEAT }, 3.5);
  regions.gain("audio-1/r", { dy: 100 });
  const region = named(f, "audio-1").children[1].state;
  expect(region).toMatchObject({ start: BAR + BEAT, file_start_seconds: 0.5, fade_in_ms: 500, fade_out_ms: 3000, gain_db: -48 });
  clips.choose(["audio-1/r"]);
  regions.step(1);
  expect(named(f, "audio-1").children[1].state.gain_db).toBe(-47);
  expect(f.steps).toEqual(["Trim a region", "Change fade in", "Change fade out", "Change gain", "Change gain"]);
});
