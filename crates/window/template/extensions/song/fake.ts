// A song for the tests of `song/`: its records, memory and clock as the window keeps them, and
// what its code asked for: each undo step by its label, what it sent, selected and rendered.
import { memory as fresh, type Memory, type Song } from "./model";

type Records = { state?: Partial<Song["state"]>; children: unknown[] };
type Piece = { tempo_map: { time_signatures: { signature: string; bars: number }[]; tempo_changes: { tick: number; bpm: number }[] }; sections: { bar: number; name: string }[] };

export function fakeSong(records: Records, { tick = 0, piece, answers = {} }: { tick?: number; piece?: Partial<Piece>; answers?: { confirm?: boolean; render?: string | null } } = {}) {
  let self = { state: { order: [], solo: [], sidechains: [], limit_off: false, ...records.state }, children: structuredClone(records.children) };
  let song: Piece = { tempo_map: { time_signatures: [{ signature: "4/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 120 }] }, sections: [], ...piece };
  const steps: string[] = [];
  const sent: [unknown, number][] = [];
  const selected: (string | null)[] = [];
  const offers: [string, string, string][] = [];
  const clock = {
    tick, playing: false, repeat: null as { from: number; to: number } | null,
    get tempo_map() { return song.tempo_map; },
    get sections() { return song.sections; },
    jump(to: number) { clock.tick = to; },
    play() { clock.playing = true; },
    setRepeat(range: { from: number; to: number } | null) { clock.repeat = range; },
    update(label: string, change: (piece: Piece) => void) {
      const draft = structuredClone(song);
      change(draft);
      song = draft;
      steps.push(label);
    },
  };
  const memory: Memory = fresh();
  const fake = {
    memory, clock,
    get state() { return self.state; },
    get children() { return self.children; },
    update(label: string, change: (draft: unknown) => void) {
      const draft = structuredClone(self);
      change(draft);
      self = draft;
      steps.push(label);
    },
    send: (input: unknown, value: number) => sent.push([input, value]),
    select: (path: string | null) => selected.push(path),
    play: () => {},
    confirm: async () => answers.confirm ?? false,
    render: async () => answers.render ?? null,
    addOffer: (key: string, path: string, label: string) => offers.push([key, path, label]),
  };
  return { song: fake as unknown as Song, memory, clock, steps, sent, selected, offers, get piece() { return song; }, get self() { return self; } };
}

/** A track `name` with a fader and the given children. */
export const trackOf = (name: string, children: unknown[] = [], automation: { lane: string; target: string }[] = []) => ({
  name, tool: "track", state: { colour: "blue", title: "", automation },
  children: [{ name: "mix", tool: "fader", state: { gain_db: 0, pan: 0, mute: false }, children: [] }, ...children],
});
/** An audio track `name` with a fader and the given children. */
export const tapeOf = (name: string, children: unknown[] = []) => ({
  name, tool: "tape", state: { colour: "teal", title: "", automation: [] },
  children: [{ name: "mix", tool: "fader", state: { gain_db: 0, pan: 0, mute: false }, children: [] }, ...children],
});
/** A clip `name` from `start`, `length` long, with `notes`, every field filled as the window gives it. */
export const clipOf = (name: string, start: number, length: number | null, notes: { start: number; length: number; pitch: number; velocity: number }[] = []) => ({
  name, tool: "timeline", children: [],
  state: { start, length, notes, loop: false, plays: true, quantize: "bar", pedal: [], bend: [], mod_wheel: [], pressure: [], controllers: {}, title: "" },
});
/** A region `name` from `start` of a file it plays `seconds` of. */
export const regionOf = (name: string, start: number, seconds: number | null = null) => ({
  name, tool: "region", children: [],
  state: { asset: `${name}.wav`, start, file_start_seconds: 0, file_end_seconds: seconds, gain_db: 0, fade_in_ms: 0, fade_out_ms: 0 },
});
/** A lane `name` with `points`. */
export const laneOf = (name: string, points: { tick: number; value: number; curve?: number }[], scale = "linear") => ({ name, tool: "lane", children: [], state: { points, scale } });
