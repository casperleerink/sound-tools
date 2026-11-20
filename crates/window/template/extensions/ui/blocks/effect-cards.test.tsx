import { expect, test } from "bun:test";
import { act, find, h, press, render, type Card, type FilledStateOfTool, type Tools } from "../../sdk";
import { drawingOf } from "../display";
import { card as analyzer } from "./cards/analyzer";
import { card as compressor } from "./cards/compressor";
import { card as delay } from "./cards/delay";
import { card as gate } from "./cards/gate";
import { card as limiter } from "./cards/limiter";
import { card as modulation } from "./cards/modulation";
import { card as pitchCorrection } from "./cards/pitch-correction";
import { card as reverb } from "./cards/reverb";
import { card as utility } from "./cards/utility";
import { card as vocoder } from "./cards/vocoder";
import { curveArea } from "./dynamics";

type Drawn = { drag?: { x?: unknown; y?: unknown } };
const dragOf = (label: string) => (find(label) as Drawn | undefined)?.drag;
/** The card of `tool` on `state` again, as after an edit from outside: its memory stays, its facts are the tool's; `held` the numbers a lane moves. */
const draw = <T extends keyof Tools>(card: Card<T, any>, tool: T, state: Partial<FilledStateOfTool<T>>, held: string[] = []) => render(card, { tool, state }, { held });
/** The card opened anew: its memory as the window first gives it. */
const open: typeof draw = (card, tool, state, held) => {
  render((() => <box />) as Card, { name: "other", state: {}, children: [] });
  return draw(card, tool, state, held);
};
/** What a press, act or drag changes, path and value, values to two decimals. */
const changes = (ops: ReturnType<typeof act>) => ops.map(([path, value]) => [path.at(-1), typeof value === "number" ? Math.round(value * 100) / 100 : value]);

const openCompressor = (held: string[] = [], state: object = {}) =>
  open(compressor, "compressor", { threshold_db: -18, ratio: 4, attack_ms: 10, release_ms: 120, knee_db: 6, makeup_db: 0, mix: 1, lookahead_ms: 0, ...state }, held);

test("compressor: the threshold's handle drags the threshold sideways, the ratio's drags the ratio up and down to two decimals", () => {
  openCompressor();
  const { w, h: tall } = curveArea();
  // The drawing is 60 dB across.
  expect(changes(act("Compressor threshold point", "drag", { dx: w / 6, dy: -40 }))).toEqual([["threshold_db", -8]]);
  openCompressor();
  // At -18 dB the ratio's handle travels one over the ratio from 1 - 60/18 at the bottom to 1 at the top:
  // a tenth up from 1/4 is 1/4 + 0.333, a ratio of 1.71.
  expect(changes(act("Compressor ratio point", "drag", { dx: 40, dy: -tall / 10 }))).toEqual([["ratio", 1.71]]);
});

test("compressor: a ratio a lane holds, or a threshold at the top, leaves the ratio's handle still", () => {
  openCompressor(["ratio"]);
  expect(dragOf("Compressor ratio point")).toBeUndefined();
  openCompressor([], { threshold_db: -0.5 });
  expect(dragOf("Compressor ratio point")).toBeUndefined();
});

test("compressor: More shows the knee, makeup, mix and lookahead; a lookahead is one pick", () => {
  openCompressor();
  expect(find("Compressor Knee")).toBeUndefined();
  press("More of Compressor");
  for (const name of ["Knee", "Makeup", "Mix"]) expect(find(`Compressor ${name}`)).toMatchObject({ role: "slider" });
  expect(find("Compressor Lookahead")).toMatchObject({ value: "0 ms" });
  expect(press("lookahead_ms-10")).toEqual([[["state", "lookahead_ms"], 10]]);
});


test("gate: the threshold's handle drags only the threshold; the hold is behind More", () => {
  open(gate, "gate", { threshold_db: -40, attack_ms: 0.5, hold_ms: 20, release_ms: 100, range_db: 80, transient_db: 0, sustain_db: 0 });
  const { w } = curveArea();
  expect(changes(act("Gate threshold point", "drag", { dx: w / 8, dy: -30 }))).toEqual([["threshold_db", -30]]);
  expect(find("Gate Hold")).toBeUndefined();
  press("More of Gate");
  expect(find("Gate Hold")).toMatchObject({ role: "slider" });
});

const limiterOf = (lookahead_ms: 0 | 1 | 5): Partial<FilledStateOfTool<"limiter">> => ({ gain_db: 0, ceiling_db: -12, release_ms: 100, lookahead_ms });

test("limiter: the ceiling's handle drags the ceiling up and down; the lookahead follows an edit from outside", () => {
  open(limiter, "limiter", limiterOf(1));
  const { h: tall } = curveArea();
  // The drawing is 24 dB tall.
  expect(changes(act("Limiter ceiling point", "drag", { dx: -30, dy: -tall / 4 }))).toEqual([["ceiling_db", -6]]);
  expect(find("Limiter Lookahead")).toMatchObject({ value: "1 ms" });
  draw(limiter, "limiter", limiterOf(5));
  expect(find("Limiter Lookahead")).toMatchObject({ value: "5 ms" });
});

test("analyzer: with no settings its card is what it hears: the level, note, peak and loudness", () => {
  open(analyzer, "analyzer", {});
  for (const name of ["level", "note", "peak", "loudness"]) expect(find(`Analyzer ${name}`)).toBeDefined();
  expect(find("More of Analyzer")).toBeUndefined();
});


test("pitch correction: the key and the scale above its display are one pick each; More shows the voice, both switches and the mix", () => {
  open(pitchCorrection, "pitch-correction", { key: "C", scale: "chromatic", retune_ms: 20, humanize: 0, voice: "middle", keep_formants: false, ignore_vibrato: false, mix: 1 });
  expect(press("key-D")).toEqual([[["state", "key"], "D"]]);
  expect(press("scale-minor")).toEqual([[["state", "scale"], "minor"]]);
  press("More of Pitch correction");
  expect(find("Pitch correction Voice")).toMatchObject({ value: "Middle" });
  expect(press("Pitch correction Formants")).toEqual([[["state", "keep_formants"], true]]);
  expect(press("Pitch correction Ignore vibrato")).toEqual([[["state", "ignore_vibrato"], true]]);
  expect(find("Pitch correction Mix")).toMatchObject({ role: "slider" });
});

const NOTES = ["1/32", "1/16", "1/8", "1/4", "1/2", "1/1"];
const delayOf = (sync: boolean): Partial<FilledStateOfTool<"delay">> => ({ sync, division: "1/8", feel: "straight", time_ms: 250, feedback: 0.4, ping_pong: false, low_cut_hz: 100, high_cut_hz: 8000, mix: 0.3 });

test("delay: the handle on the second repeat drags the time and the feedback as one step", () => {
  open(delay, "delay", delayOf(false));
  const { w, h: tall } = drawingOf(4, true);
  // The second repeat is two gaps along: the time's whole travel is 0.8 of the drawing; the feedback 0.88 of it.
  const ops = act("Delay time and feedback", "drag", { dx: w * 0.08, dy: -tall * 0.088 });
  expect(ops.map(([path]) => path.at(-1))).toEqual(["time_ms", "feedback"]);
  expect(Math.round(ops[0]![1] as number)).toBe(Math.round(250 * 4000 ** 0.1));
  expect(changes(ops)[1]).toEqual(["feedback", 0.5]);
  expect(find("Delay Feel")).toBeUndefined();
});

test("delay: while it syncs the time is a note, which the handle steps through, and the feel shows", () => {
  open(delay, "delay", delayOf(true));
  const { w } = drawingOf(4, true);
  expect(find("Delay Time")).toMatchObject({ value: "1/8" });
  expect(changes(act("Delay time and feedback", "drag", { dx: (w * 0.8) / (NOTES.length - 1) }))).toEqual([["division", "1/4"]]);
  expect(press("Delay Feel Dotted")).toEqual([[["state", "feel"], "dotted"]]);
  expect(press("Delay Sync")).toEqual([[["state", "sync"], false]]);
});

test("delay: More shows the cuts and ping-pong", () => {
  open(delay, "delay", delayOf(false));
  press("More of Delay");
  expect(find("Delay Low cut")).toMatchObject({ role: "slider" });
  expect(find("Delay High cut")).toMatchObject({ role: "slider" });
  expect(press("Delay Ping-pong")).toEqual([[["state", "ping_pong"], true]]);
});


test("reverb: the handles drag the pre-delay and the decay, each sideways; captions say them while their knobs are hidden", () => {
  open(reverb, "reverb", { pre_delay_ms: 20, decay_seconds: 2, size: 0.5, damping: 0.5, diffusion: 0.7, low_cut_hz: 100, high_cut_hz: 8000, width: 1, mix: 0.3, freeze: false });
  const { w } = drawingOf(4);
  expect(JSON.stringify(find("Reverb decay point"))).toContain("Decay 2 s");
  // The pre-delay's whole travel is 0.14 of the drawing, the decay's 0.74.
  const pre = act("Reverb pre-delay point", "drag", { dx: w * 0.014, dy: 30 });
  expect(changes(pre)).toEqual([["pre_delay_ms", Math.round(20 * 500 ** 0.1 * 100) / 100]]);
  const decay = act("Reverb decay point", "drag", { dx: -w * 0.074, dy: -30 });
  expect(changes(decay)).toEqual([["decay_seconds", Math.round(2 * 300 ** -0.1 * 100) / 100]]);
  press("More of Reverb");
  expect(JSON.stringify(find("Reverb decay point"))).not.toContain("Decay 1.13 s");
  for (const name of ["Low cut", "High cut", "Diffusion", "Pre-delay", "Decay"]) expect(find(`Reverb ${name}`)).toMatchObject({ role: "slider" });
  expect(press("Reverb Freeze")).toEqual([[["state", "freeze"], true]]);
});


test("modulation: the handles drag the depth up and the spread sideways; a mode is one click; More shows the spread", () => {
  open(modulation, "modulation", { mode: "chorus", rate_hz: 0.5, depth: 0.5, feedback: 0.2, spread: 0.5, mix: 0.5 });
  const { w, h: tall } = drawingOf(4, true);
  // Full depth swings 0.36 of the drawing up; the spread's whole travel is a quarter across.
  expect(changes(act("Modulation depth point", "drag", { dx: 30, dy: -tall * 0.18 }))).toEqual([["depth", 1]]);
  expect(changes(act("Modulation spread point", "drag", { dx: -w / 8, dy: 30 }))).toEqual([["spread", 0]]);
  expect(press("Modulation Mode Flanger")).toEqual([[["state", "mode"], "flanger"]]);
  expect(find("Modulation Spread")).toBeUndefined();
  press("More of Modulation");
  expect(find("Modulation Spread")).toMatchObject({ role: "slider" });
});

test("vocoder: the bands are whole", () => {
  open(vocoder, "vocoder", { bands: 24, formant: 0, release_ms: 60, mix: 1 });
  expect(act("Vocoder Bands", "set", 30.4)).toEqual([[["state", "bands"], 30]]);
});

const utilityOf = (state: Partial<FilledStateOfTool<"utility">> = {}): Partial<FilledStateOfTool<"utility">> => ({ gain_db: 0, pan: 0, width: 1, bass_mono: false, bass_mono_hz: 120, channels: "stereo", invert_left: false, invert_right: false, mute: false, ...state });

test("utility: the handle drags gain and pan as one step; the channels read left to right", () => {
  open(utility, "utility", utilityOf());
  const { w, h: tall } = drawingOf(4, true);
  // The pan's travel is a sound twice as wide as the speakers each side, the gain's 72 dB over 0.64 of the drawing.
  const widest = Math.atan(2) / (Math.PI / 4);
  expect(changes(act("Utility gain and pan", "drag", { dx: (w * 0.5) / (2 * widest), dy: (-tall * 0.64) / 6 }))).toEqual([["gain_db", 12], ["pan", 0.5]]);
  expect((find("Utility Channels") as { children?: { label?: string }[] } | undefined)?.children?.map((option) => option.label)).toEqual(["Utility Channels Left", "Utility Channels Stereo", "Utility Channels Right", "Utility Channels Swap"]);
});

test("utility: More shows bass mono, below and the inverts, and a switch is one click", () => {
  open(utility, "utility", utilityOf());
  expect(press("Utility Mute")).toEqual([[["state", "mute"], true]]);
  press("More of Utility");
  expect(find("Utility Below")).toMatchObject({ role: "slider" });
  expect(press("Utility Bass mono")).toEqual([[["state", "bass_mono"], true]]);
  expect(press("Utility Invert left")).toEqual([[["state", "invert_left"], true]]);
  expect(press("Utility Invert right")).toEqual([[["state", "invert_right"], true]]);
});
