import { Arc, boolean, Button, Circle, Path, DeviceCard, For, TitleBar, Toggle, Icon, Knob, PeakMeter, type Color, Fragment, Group, h, Input, Label, Line, list, memo, now, Rect, selector, Show, signal, Space, text, tool, Transport, untrack, Wave, type Accessor, type Connection, type Declaration, type Drag, type Dropped, type FilledStateOfTool, type Frozen, type Context, type Inputs, type Key, type KeyName, type Live, type MovedNumber, type OwnedChild, type Part, type Press, type TextPart } from "./sdk";
import { bus } from "./bus";
import { chain } from "./chain";
import { ClipEditor, editorKey, lengthOf, snapsOf, type EditorMemory, type Lane } from "./clip-editor";
import { freeName, tape } from "./tape";
import { splitTarget, track } from "./track";

// A song: tracks of clips in time, with their automation, a mixer strip per track, the devices
// of the chosen track, and a note editor for one clip.
/** A name or a field in plain words, its unit left out: `osc_1.position` is `Osc 1 position`, `cutoff_hz` is `Cutoff`. */
const words = (text: string) =>
  text.replace(/_(hz|db|ms|seconds|cents|octaves|semitones)$/, "").replace(/[._-]+/g, " ").trim().replace(/^./, (first) => first.toUpperCase());

const BEAT = 960;
// The song shows at least 64 bars of 4/4, and 32 more after what it holds.
const SHOWN = 64 * 4 * BEAT;
const AFTER = 32 * 4 * BEAT;
// As the window drew a song before it was a tool: rows of 64 points, headers 176 wide, a ruler
// 32 tall, and bar 1 eight points right of the headers.
const ROW = 64;
const HEADER = 176;
const RULER = 32;
const MASTER_ROW = 40;
// The row under a track's lanes that adds one.
const ADD_LANE = 40;
const EDITOR = 352;
// The panel of the chosen track, as tall as a card and the room around it.
/** A device card's height, and the panel's: its cards, 11 points above them and 12 under. */
const CARD = 192;
const RACK = CARD + 23;
// Points per tick at zoom 1: a bar is 96 points.
const ACROSS = 0.025;
// The ticks left of bar 1, eight points at zoom 1.
const LEAD = 320;
// The pitches a clip's picture spans.
const [LOW, HIGH] = [36, 96];

const COLOR = {
  back: "#121317", lane: "#17191e", bar: "#2a2d35", text: "#7c808c", chosen: "#e9ebef", playhead: "#7ee0a0",
  note: "#00000080", marquee: "#a9b1ff33", off: "#2a2d35", mute: "#f0b44c", solo: "#7ee0a0", meter: "#7ee0a0", clear: "#00000000",
  edit: "#a9b1ff", record: "#ff6b6b", repeat: "#a9b1ff55",
} as const;
const PALETTE = [
  { name: "Blue", colour: "blue" }, { name: "Peach", colour: "peach" }, { name: "Mauve", colour: "mauve" }, { name: "Teal", colour: "teal" },
  { name: "Green", colour: "green" }, { name: "Yellow", colour: "yellow" }, { name: "Pink", colour: "pink" }, { name: "Sky", colour: "sky" },
  { name: "Sapphire", colour: "sapphire" }, { name: "Red", colour: "red" }, { name: "Maroon", colour: "maroon" }, { name: "Lavender", colour: "lavender" },
  { name: "Rosewater", colour: "rosewater" }, { name: "Flamingo", colour: "flamingo" },
] as const;

const [MOVES, RESIZES, START_MOVES] = [["move"], ["resize"], ["resize-start"]];
const parts = { master: "fader", limit: "limiter", quiet: "lane", "audio-*": tape, "ret-*": bus, "*": track } as const;
const STRIP = 72;
const MIXER_FADER = 140;
/** A knob with its name and value under it. */
const KNOB = 72;
// The fader of the chosen track's panel.
const PANEL_FADER = 113;

type Clipboard = { from: number; clips: { row: number; track: string; state: FilledStateOfTool<"timeline"> }[] };
/** A track of the song as code reads it. */
type TrackRow = Extract<Frozen<OwnedChild<typeof parts>>, { tool: typeof track }>;
/** A copy of a clip's state to change, from a view's record or a key's. */
const copyOf = (state: Frozen<FilledStateOfTool<"timeline">>) => JSON.parse(JSON.stringify(state)) as FilledStateOfTool<"timeline">;
type Editing = { kind: "tempo" | "signature" | "section"; bar: number } | null;
/** What shows under the timeline, one at a time: a track's panel, the master's, a clip's notes or the mixer. */
type Below = { kind: "track"; name: string } | { kind: "master" } | { kind: "editor"; clip: string } | { kind: "mixer" } | null;

type Memory = {
  /** The chosen clips, `track/clip`, and the chosen region. */
  selected: Set<string>;
  region: string | null;
  below: Below;
  armed: Set<string>;
  recording: number | null;
  /** The track the keyboard plays, and the one whose name is typed. */
  track: string | null;
  renaming: string | null;
  renamingClip: string | null;
  clipboard: Clipboard | null;
  /** The tracks whose automation lanes show under them, and each lane's range by `track/target`. */
  lanes: Set<string>;
  ranges: Record<string, MovedNumber>;
  editing: Editing;
  /** The range the ruler shows to repeat, while the clock does not repeat. */
  repeatRange: { from: number; to: number };
  /** Whether the returns show a row each, or fold to one. */
  returnsOpen: boolean;
  /** What each device of a track is called, by `track/path`, as its card's title says: for the lanes' names. */
  names: Record<string, string>;
  /** The chosen point of an automation lane, by its tick, which delete removes. */
  point: { track: string; lane: string; tick: number } | null;
} & EditorMemory;

const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);
// Where a level sits on a meter or a fader, 0 to 1: its amplitude to this power, 0 dB at 0.8.
const [CURVE, ZERO_DB] = [20 / 61.94, 0.8];

// Bars: the time signatures are runs of bars, and the last run goes on.
type Runs = readonly { readonly signature: string; readonly bars: number }[];
const ticksOfBar = (signature: string) => {
  const [count, unit] = signature.split("/").map(Number);
  return ((count ?? 4) * 4 * BEAT) / (unit ?? 4);
};
/** The signature of each of `count` bars. */
function signaturesOf(runs: Runs, count: number): string[] {
  const each: string[] = [];
  runs.forEach((run, index) => {
    const bars = index === runs.length - 1 ? Math.max(count - each.length, 0) : run.bars;
    for (let bar = 0; bar < bars; bar++) each.push(run.signature);
  });
  return each.slice(0, count);
}
/** The runs of `signatures`, one a bar. */
function runsOf(signatures: string[]): { signature: string; bars: number }[] {
  const runs: { signature: string; bars: number }[] = [];
  for (const signature of signatures) {
    const last = runs.at(-1);
    if (last?.signature === signature) last.bars += 1;
    else runs.push({ signature, bars: 1 });
  }
  return runs;
}
/** The tick each bar starts at, up to the first at or past `end`. */
function barStarts(runs: Runs, end: number): number[] {
  const starts = [0];
  let at = 0;
  const all = runs.length > 0 ? runs : [{ signature: "4/4", bars: 1 }];
  all.forEach((run, index) => {
    const goesOn = index === all.length - 1;
    for (let bar = 0; goesOn ? at < end : bar < run.bars; bar++) {
      at += ticksOfBar(run.signature);
      starts.push(at);
    }
  });
  return starts;
}

/** Ticks as seconds of a file at `bpm`. */
const secondsOf = (ticks: number, bpm: number) => (ticks / BEAT) * (60 / bpm);

type TempoChanges = readonly { tick: number; bpm: number }[];

/** The tempo at `tick`, and the tick of the next change after it. */
function tempoAt(changes: TempoChanges, tick: number): { bpm: number; next: number } {
  const bpm = [...changes].reverse().find((change) => change.tick <= tick)?.bpm ?? 120;
  return { bpm, next: changes.find((change) => change.tick > tick)?.tick ?? Infinity };
}

/** The seconds from `from` to `to`, through each tempo between: below 0 when `to` is before. */
function secondsBetween(changes: TempoChanges, from: number, to: number): number {
  if (to < from) return -secondsBetween(changes, to, from);
  let seconds = 0;
  for (let tick = from; tick < to; ) {
    const { bpm, next } = tempoAt(changes, tick);
    const end = Math.min(next, to);
    seconds += secondsOf(end - tick, bpm);
    tick = end;
  }
  return seconds;
}

/** The ticks `seconds` take from `from`, through each tempo after it. */
function ticksAcross(changes: TempoChanges, from: number, seconds: number): number {
  let [tick, left] = [from, seconds];
  while (left > 0) {
    const { bpm, next } = tempoAt(changes, tick);
    const fits = secondsOf(next - tick, bpm);
    if (left <= fits) return tick + (left * bpm * BEAT) / 60 - from;
    [tick, left] = [next, left - fits];
  }
  return tick - from;
}


/** A child's name from any text: lowercase letters, digits and `-`. */
const nameFrom = (text: string) => text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "x";

/** Where a value sits on a lane, 0 to 1, as its knob travels its range, and back. */
function upOf(range: MovedNumber, value: number): number {
  if (range.scale === "log") return Math.log(value / range.min) / Math.log(range.max / range.min);
  if (range.scale === "fader") return value <= -100 ? 0 : clamp(Math.pow(Math.pow(10, value / 20), CURVE) * ZERO_DB, 0, 1);
  return (value - range.min) / (range.max - range.min);
}
function valueAt(range: MovedNumber, up: number): number {
  const at = clamp(up, 0, 1);
  if (range.scale === "log") return range.min * Math.pow(range.max / range.min, at);
  if (range.scale === "fader") return at <= 0 ? -100 : Math.min(20 * Math.log10(Math.pow(at / ZERO_DB, 1 / CURVE)), range.max);
  return range.min + at * (range.max - range.min);
}

function Dial({ path, min, max, label, travel }: { path: string; min: number; max: number; label: string; travel?: number }) {
  const angle = now.value(path, { played: true }).minus(min).over(max - min).max(0).min(1).minus(0.5).times(0.75);
  return (
    <div style={{ width: 52, gap: 2, align: "center" }}>
      <Space key={`dial-${path}`} width={28} height={28} cursor="ns-resize" drag={{ y: { value: path, min, max, travel: travel ?? 200, label: `Change ${label.toLowerCase()}` } }}>
        <Arc x={14} y={14} radius={10} from={-0.375} to={0.375} color={COLOR.lane} width={3} />
        <Arc x={14} y={14} radius={10} from={-0.375} to={angle} color={COLOR.edit} width={3} />
      </Space>
      <div style={{ fontSize: 9, color: COLOR.text }}>{label}</div>
    </div>
  );
}

export const song = tool({
  name: "song",
  title: "Song",
  when: "You change the song page: its tracks, clips, automation, mixer strips or note editor",
  doc:
    "Tracks of clips in time, and the devices of the chosen track under them. Each `track` child and each audio track `audio-*` plays into the fader `master`, then its limiter `limit` unless `limit_off`; `order` is the order of the tracks, `solo` names the tracks that play alone. A track's R arms its recorder `rec`. The key R or the record button records from the playhead until pressed again or a stop: every armed track, the audio input into new regions, and the keyboard's notes into a new clip of the chosen track, or of the first while none is chosen, which the keyboard plays then. Drag a clip to move it (alt copies it, up and down moves it to another track; on its own track the automation under it goes along), drag its right end to resize it, double-click empty space to add one and a clip to edit its notes below, drag over empty space to choose the clips there; right-click it to rename, copy or delete it. Cmd-C, cmd-X, cmd-V, cmd-D, delete and the arrows work on the chosen clips, or on the chosen notes while the editor has some; cmd-V pastes copied notes while the editor is open, alt-up and alt-down step the chosen velocities, and escape closes it. A track's menu (right-click its name) renames it, colours it, shows its automation, adds a lane for a number of its devices, or deletes it; drag a track's head onto another to move it there. A lane: click to add a point, chosen, drag one to move it, delete removes the chosen one and alt-click any. The returns sit over the master row, each with a panel of its effects; a track's panel ends with a knob per send. The ruler: click to play from there, double-click to add a section, right-click to add a tempo change, a time signature or a section, drag over it to repeat a range; click the repeat's bar at its foot to turn it on or off, drag it or an end to move it. A region: drag to move it, an edge to trim it; its gain and fades under the timeline. Space plays and pauses, wherever the focus is. Each of `sidechains` keys an effect, by its path such as `t00/chain/comp`, with what a track `from` plays after its fader.",
  state: { order: list(text(), { max: 128 }), solo: list(text(), { max: 64 }), sidechains: list({ effect: text(), from: text() }, { max: 64 }), limit_off: boolean({ default: false }) },
  children: parts,
  memory: (): Memory => ({
    selected: new Set(), region: null, below: null, armed: new Set(), recording: null, track: null, renaming: null, renamingClip: null,
    clipboard: null, lanes: new Set(), ranges: {}, editing: null, repeatRange: { from: 0, to: 4 * 4 * BEAT }, returnsOpen: false, point: null, names: {},
    notes: new Set(), snap: BEAT / 4, lane: "velocity", length: BEAT / 2, copied: null,
  }),
  keys: ["delete", "backspace", "cmd-a", "cmd-c", "cmd-x", "cmd-v", "cmd-d", "left", "right", "up", "down", "alt-up", "alt-down", "escape", "r"],
  globalKeys: ["space"],
  onKey: (tool, key) => {
    const { state, memory, update, clock, children } = tool;
    const order = (name: string) => (state.order.includes(name) ? state.order.indexOf(name) : state.order.length);
    if (key.key === "space") {
      if (key.down) (clock.playing ? clock.pause() : clock.play());
      return;
    }
    if (!key.down) return;
    if (key.key === "r") {
      toggleRecordingOf(tool, state.order);
      return;
    }
    // The editor's notes first, while it has some chosen; escape with none closes it.
    const opened = memory.below?.kind === "editor" ? memory.below.clip : null;
    if (opened && key.key === "escape" && memory.notes.size === 0) {
      memory.below = { kind: "track", name: opened.split("/")[0] ?? "" };
      return;
    }
    if (opened) {
      const [trackName, clipName] = opened.split("/");
      const row = children.find((each) => each.name === trackName);
      const clip = row?.tool === "track" ? row.children.find((child) => child.name === clipName) : undefined;
      const key2 = opened;
      const edit = (label: string, change: (clip: FilledStateOfTool<"timeline">) => void) =>
        update(label, ({ children }) => {
          const owner = children.find((each) => each.name === key2.split("/")[0]);
          const draft = owner?.tool === "track" ? owner.children.find((child) => child.name === key2.split("/")[1]) : undefined;
          if (draft?.tool === "timeline") change(draft.state);
        });
      const forNotes = memory.notes.size > 0 || (key.cmd && (key.key === "a" || (key.key === "v" && memory.copied !== null)));
      const state = clip?.tool === "timeline" ? clip.state : undefined;
      if (forNotes && editorKey(key, state, clock.tick, memory, edit)) return;
    }
    if (memory.point && (key.key === "delete" || key.key === "backspace" || key.key === "escape")) {
      const { track: owner, lane, tick } = memory.point;
      memory.point = null;
      if (key.key === "escape") return;
      // The last point takes its lane along.
      update("Remove a point", ({ children }) => {
        const row = children.find((child) => child.name === owner);
        const points = row?.tool === "track" ? row.children.find((child) => child.name === lane) : undefined;
        if (row?.tool !== "track" || points?.tool !== "lane") return;
        const rest = points.state.points.filter((point) => point.tick !== tick);
        if (rest.length > 0) points.state.points = rest;
        else {
          row.children = row.children.filter((child) => child !== points);
          row.state.automation = row.state.automation.filter((each) => each.lane !== lane);
        }
      });
      return;
    }
    if (memory.region && (key.key === "delete" || key.key === "backspace")) {
      const [owner, name] = memory.region.split("/");
      memory.region = null;
      update("Remove a region", ({ children }) => {
        const tape = children.find((child) => child.name === owner);
        if (tape?.tool === "tape") tape.children = tape.children.filter((child) => child.name !== name);
      });
      return;
    }
    // Alt-up and alt-down step the chosen region's gain by a decibel.
    if (memory.region && key.alt && (key.key === "up" || key.key === "down")) {
      const [owner, name] = memory.region.split("/");
      const by = key.key === "up" ? 1 : -1;
      update("Change gain", ({ children }) => {
        const tape = children.find((child) => child.name === owner);
        const region = tape?.tool === "tape" ? tape.children.find((child) => child.name === name) : undefined;
        if (region?.tool === "region") region.state.gain_db = clamp(Math.round((Math.max(region.state.gain_db, -48) + by) * 10) / 10, -48, 24);
      });
      return;
    }
    const tracks = children
      .filter((child): child is TrackRow => child.tool === "track")
      .slice()
      .sort((a, b) => order(a.name) - order(b.name) || (a.name < b.name ? -1 : 1));
    clipKey(key, tracks, tool);
  },
  behaviour: ({ state, children }): Declaration<typeof parts> => {
    const tracks = children.filter((child) => child.tool === "track" || child.tool === "tape");
    const returns = children.filter((child) => child.tool === "bus");
    const soloed = state.solo.filter((name) => tracks.some((each) => each.name === name));
    const connections: Connection<typeof parts>[] = [
      ...[...tracks, ...returns].map((each) => ({ from: { child: each.name }, to: { child: "master" } })),
      ...(state.limit_off
        ? [{ from: { child: "master" }, to: { port: "audio" } } as const]
        : [{ from: { child: "master" }, to: { child: "limit" } } as const, { from: { child: "limit" }, to: { port: "audio" } } as const]),
    ];
    // What the song hears, the keyboard while nothing is chosen, plays its first track.
    const first = notesTrackOf(children, state.order, null);
    if (first) connections.push({ from: { notes: "notes" }, to: { child: first, notes: "notes" } });
    // A solo mutes every other track's fader, gliding, so a lane that moves its volume plays on under it; a return always plays.
    const soloedOut = (name: string) => soloed.length > 0 && !soloed.includes(name);
    for (const each of tracks) {
      if (soloedOut(each.name)) connections.push({ from: { child: "quiet", number: "value" }, to: { child: `${each.name}/mix` as const, number: "mute" } });
    }
    // A send of a track plays into its return: after the track's fader, or before it for a return that says so. A muted or soloed-out track sends nothing either way.
    for (const each of tracks) {
      const mix = each.children.find((child) => child.name === "mix");
      const silent = soloedOut(each.name) || (mix?.tool === "fader" && mix.state.mute === true);
      for (const back of returns) {
        if (back.tool !== "bus" || !each.children.some((child) => child.name === `send-${back.name}`)) continue;
        const from = back.state.pre_fader ? "chain" : "mix";
        const send = `${each.name}/send-${back.name}` as const;
        connections.push({ from: { child: `${each.name}/${from}` as const }, to: { child: send } });
        connections.push({ from: { child: send }, to: { child: back.name } });
        if (back.state.pre_fader && silent) connections.push({ from: { child: "quiet", number: "value" }, to: { child: send, number: "mute" } });
      }
    }
    // A sidechain keys an effect of one track with another track, after its fader.
    for (const { effect, from } of state.sidechains) {
      if (!tracks.some((each) => each.name === from)) continue;
      connections.push({ from: { child: `${from}/mix` as const }, to: { child: effect as `${string}/${string}`, port: "sidechain" } });
    }
    return { connections };
  },
  view: ({ state, children, memory, update, clock, select, send, play, openMenu, offers, addOffer, numbers, outputs }) => {
    type Child = (typeof children)[number];
    const rank = (name: string) => {
      const at = state.order.indexOf(name);
      return at < 0 ? state.order.length : at;
    };
    // The rows of the timeline: the tracks and the audio tracks, in `order`.
    const rows = memo(() =>
      children.filter((child) => child.tool === "track" || child.tool === "tape").slice().sort((a, b) => rank(a.name) - rank(b.name) || (a.name < b.name ? -1 : 1)),
    );
    const tracks = () => rows().filter((each): each is TrackRow => each.tool === "track");
    const returns = () => children.filter((child) => child.tool === "bus");
    /** What has a panel: the tracks and the returns. */
    const strips = () => [...rows(), ...returns()];
    const lanesOf = (each: Child) =>
      each.tool === "track"
        ? each.state.automation.flatMap(({ lane, target }) =>
            each.children.some((child) => child.name === lane && child.tool === "lane") ? [{ lane, target }] : [],
          )
        : [];
    type Row = { each: Child; lane: { lane: string; target: string } | null; add?: true };
    // A row of the timeline for each track, and for a track that shows its lanes, one for each
    // lane and one, shorter, to add a lane.
    const layout = memo((): Row[] =>
      rows().flatMap((each) => [
        { each, lane: null },
        ...(memory.lanes.has(each.name) ? [...lanesOf(each).map((lane) => ({ each, lane })), { each, lane: null, add: true as const }] : []),
      ]),
    );
    // Where each row starts, in rows of a track, and where they all end.
    const heightOf = (row: Row) => (row.add ? ADD_LANE / ROW : 1);
    const tops = memo(() => layout().reduce<number[]>((all, row, index) => [...all, (all[index] ?? 0) + heightOf(row)], [0]));
    const topOf = (index: number) => tops()[index] ?? 0;
    /** The row at `y`, as its place in the layout. */
    const rowAt = (y: number) => Math.max(tops().findIndex((top) => top > y) - 1, 0);
    const regionsOf = (each: Child) => (each.tool === "tape" ? each.children.filter((child) => child.tool === "region") : []);
    /** The channels of the audio input the recorder of an audio track records. */
    const inputOf = (each: Child | undefined): readonly number[] => {
      const rec = each?.tool === "tape" ? each.children.find((child) => child.name === "rec") : undefined;
      return rec?.tool === "recorder" ? [...rec.state.channels] : [];
    };
    const clipsOf = (each: Child) => (each.tool === "track" ? each.children.filter((child) => child.tool === "timeline") : []);
    const recordable = (each: Child) => (each.tool === "track" || each.tool === "tape") && each.children.some((child) => child.name === "rec");
    const runs = () => clock.tempo_map.time_signatures;
    // As far as the song shows: past what it holds, and past where it repeats.
    const end = memo(() => {
      const clips = rows().flatMap((each) => clipsOf(each).map((clip) => clip.state.start + lengthOf(clip.state)));
      const regions = rows().flatMap((each) => regionsOf(each).map((region) => region.state.start));
      const lanes = rows().flatMap((each) => (each.tool === "track" ? each.children : []).flatMap((child) => (child.tool === "lane" ? child.state.points.map((point) => point.tick) : [])));
      return Math.max(SHOWN, ...clips, ...regions, ...lanes, clock.repeat?.to ?? 0) + AFTER;
    });
    // Where what the song holds ends: the strip of the transport goes this far.
    const held = memo(() => {
      const clips = rows().flatMap((each) => clipsOf(each).map((clip) => clip.state.start + lengthOf(clip.state)));
      const regions = rows().flatMap((each) => regionsOf(each).map((region) => region.state.start));
      return Math.max(0, ...clips, ...regions);
    });
    const starts = memo(() => barStarts(runs(), end()));
    /** The bar `tick` is in, from 1. */
    const barOf = (tick: number) => Math.max(starts().findIndex((start) => start > tick), 1);

    const toggleRecording = () => toggleRecordingOf({ memory, send, clock, children }, state.order);
    // A pause or a stop ends the recording, as in the arrangement.
    memo(() => {
      if (!clock.playing && untrack(() => memory.recording) !== null) untrack(toggleRecording);
    });
    // An armed track's input is open: its level shows, and a take has its sound from the first frame.
    const toggleArm = (name: string) => {
      const armed = new Set(memory.armed);
      const on = !armed.delete(name);
      if (on) armed.add(name);
      memory.armed = armed;
      send({ child: `${name}/rec` as const, number: "arm" }, on ? 1 : 0);
      if (memory.recording !== null) send({ child: `${name}/rec` as const, number: "record" }, on ? 1 : 0);
    };
    const isPicked = selector(() => memory.selected);
    const clipAt = (key: string) => {
      const [trackName, clipName] = key.split("/");
      const clip = tracks().find((each) => each.name === trackName)?.children.find((child) => child.name === clipName);
      return clip?.tool === "timeline" ? clip : undefined;
    };
    const openKey = () => (memory.below?.kind === "editor" ? memory.below.clip : null);
    const panelOf = () => (memory.below?.kind === "track" ? memory.below.name : null);
    const open = () => {
      const key = openKey();
      return key ? clipAt(key) : undefined;
    };
    /** Chooses the clips `keys` names, `track/clip`: the keyboard plays the track of the first, or `track`, and Export selection exports their bars. */
    const choose = (keys: Set<string>, track: string | null = null) => {
      memory.selected = keys;
      memory.region = null;
      memory.point = null;
      memory.track = ([...keys][0] ?? track)?.split("/")[0] ?? null;
      const clips = [...keys].flatMap((key) => clipAt(key) ?? []);
      const starts = clips.map((clip) => clip.state.start);
      const ends = clips.map((clip) => clip.state.start + lengthOf(clip.state));
      const span = clips.length === 0 ? undefined : { from: Math.min(...starts), to: Math.max(...ends) };
      select([...keys][0] ?? track, { span });
    };
    /** Shows the panel of the track `name` under the timeline, and gives it the keyboard. */
    const openPanel = (name: string) => {
      choose(new Set(), name);
      memory.below = { kind: "track", name };
    };
    const openClip = (key: string) => {
      memory.below = { kind: "editor", clip: key };
      memory.notes = new Set();
    };

    /** Changes the clips `keys` names, `track/clip`, in one undo step. */
    const editClips = (label: string, keys: Iterable<string>, change: (state: FilledStateOfTool<"timeline">) => void) =>
      update(label, ({ children }) => {
        for (const key of keys) {
          const [trackName, clipName] = key.split("/");
          const row = children.find((each) => each.name === trackName);
          const clip = row?.tool === "track" ? row.children.find((child) => child.name === clipName) : undefined;
          if (clip?.tool === "timeline") change(clip.state);
        }
      });

    // The clips' drags land on the grid of the snap in the corner, a tick apart while it is off.
    const moveDrag = (snap: number): Drag => ({
      id: "move",
      snap: { x: snap || 1, y: 1, grid: snap > 0 },
      onEnd: ({ dx, dy, alt }) => choose(moveClips(update, layout(), memory.selected, dx, Math.round(dy), alt)),
    });
    const marquee = now.drag("clip-marquee");
    const marqueeDrag: Drag = {
      id: "clip-marquee",
      onEnd: ({ startX, startY, x, y, shift, cmd }) => {
        const [left, right] = [Math.min(startX, x), Math.max(startX, x)];
        const [top, bottom] = [Math.min(startY, y), Math.max(startY, y)];
        const inside = layout().flatMap(({ each, lane, add }, row) =>
          lane || add || topOf(row) + 1 <= top || topOf(row) >= bottom
            ? []
            : clipsOf(each)
                .filter(({ state }) => state.start < right && state.start + lengthOf(state) > left)
                .map((clip) => `${each.name}/${clip.name}`),
        );
        choose(new Set([...(shift || cmd ? memory.selected : []), ...inside]));
      },
    };
    const resizeDrag = (snap: number): Drag => ({
      id: "resize",
      axis: "x",
      snap: { x: snap || 1, grid: snap > 0 },
      onEnd: ({ dx }) =>
        editClips("Resize clips", memory.selected, (clip) => {
          const length = Math.max(lengthOf(clip) + dx, snap || BEAT / 4);
          // What starts at or past its new end is cut: a timeline plays nothing there.
          clip.length = length;
          clip.notes = clip.notes.filter((note) => note.start < length);
          clip.pedal = clip.pedal.filter((change) => change.start < length);
          clip.bend = clip.bend.filter((point) => point.tick < length);
          clip.mod_wheel = clip.mod_wheel.filter((point) => point.tick < length);
          clip.pressure = clip.pressure.filter((point) => point.tick < length);
        }),
    });
    const resizeStartDrag = (snap: number): Drag => ({
      id: "resize-start",
      axis: "x",
      snap: { x: snap || 1, grid: snap > 0 },
      onEnd: ({ dx }) => editClips("Resize clips", memory.selected, (clip) => startMoved(clip, dx, snap || BEAT / 4)),
    });

    // As the arrangement's: what the keys do, and the name.
    const CLIP_MENU: { label: string; command?: ClipCommand; shortcut?: KeyName; separator?: true }[] = [
      { label: "Cut", command: "cut", shortcut: "cmd-x" }, { label: "Copy", command: "copy", shortcut: "cmd-c" }, { label: "Duplicate", command: "duplicate", shortcut: "cmd-d" },
      { label: "Rename", separator: true }, { label: "Delete", command: "delete", shortcut: "backspace", separator: true },
    ];
    const clipMenu = async (key: string) => {
      if (!memory.selected.has(key)) choose(new Set([key]));
      const picked = await openMenu(CLIP_MENU.map(({ label, shortcut, separator }) => ({ label, shortcut, separator })), { width: 240 });
      const item = picked === null ? undefined : CLIP_MENU[picked];
      if (item?.command) clipCommand(item.command, tracks(), { memory, update, select });
      else if (item) memory.renamingClip = key;
    };
    const pressClip = (key: string) => ({ clicks, shift, cmd, button }: Press) => {
      if (button === "right") {
        clipMenu(key);
      } else if (clicks === 2) {
        openClip(key);
      } else if (shift || cmd) {
        const next = new Set(memory.selected);
        if (!next.delete(key)) next.add(key);
        choose(next);
      } else if (!memory.selected.has(key)) {
        choose(new Set([key]));
      }
    };
    const pressTimeline = ({ clicks, x, y }: Press) => {
      const index = rowAt(y);
      const row = layout()[index];
      if (row?.lane) {
        if (clicks === 1) addPoint(row.each.name, row.lane, x, 1 - (y - topOf(index) - 0.1) / 0.8, cmd);
        return;
      }
      if (clicks !== 2 || row?.add || row?.each.tool !== "track") {
        choose(new Set());
        return;
      }
      // A bar long, in the cell of the grid under the pointer.
      const start = Math.max(memory.snap > 0 ? Math.floor(x / memory.snap) * memory.snap : Math.round(x), 0);
      const bar = barOf(start);
      const length = (starts()[bar] ?? 0) - (starts()[bar - 1] ?? 0) || 4 * BEAT;
      let added: string | undefined;
      update("Add a clip", ({ children }) => {
        const owner = children.find((child) => child.name === row.each.name);
        if (owner?.tool !== "track") return;
        const name = freeName(owner.children.map((child) => child.name), "clip");
        owner.children = [...owner.children, { name, tool: "timeline", state: { start, length, notes: [] } }];
        added = `${owner.name}/${name}`;
      });
      if (added) choose(new Set([added]));
    };
    const dropOnTimeline = ({ files, x, y }: Dropped) => {
      const row = layout()[rowAt(y)]?.each;
      const owner = row?.tool === "tape" ? row : rows().find((each) => each.tool === "tape");
      if (!owner || files.length === 0) return;
      const start = Math.max(Math.round(x / BEAT) * BEAT, 0);
      update("Add audio", ({ children }) => {
        const tape = children.find((child) => child.name === owner.name);
        if (tape?.tool !== "tape") return;
        for (const file of files) {
          const name = freeName(tape.children.map((child) => child.name), "region");
          tape.children = [...tape.children, { name, tool: "region", state: { asset: file, start } }];
        }
      });
    };

    // Tracks
    const titleOf = (each: Child) => ((each.tool === "track" || each.tool === "tape" || each.tool === "bus") && each.state.title) || each.name;
    const rename = (name: string, title: string) => {
      memory.renaming = null;
      update("Rename a track", ({ children }) => {
        const owner = children.find((child) => child.name === name);
        if (owner?.tool === "track" || owner?.tool === "tape" || owner?.tool === "bus") owner.state.title = title;
      });
    };
    const mixOf = (each: Child) => (each.tool === "track" || each.tool === "tape" || each.tool === "bus" ? each.children.find((child) => child.name === "mix") : undefined);
    const toggleMute = (name: string) =>
      update("Mute a track", ({ children }) => {
        const owner = children.find((child) => child.name === name);
        const mix = owner?.tool === "track" || owner?.tool === "tape" || owner?.tool === "bus" ? owner.children.find((child) => child.name === "mix") : undefined;
        if (mix?.tool === "fader") mix.state.mute = !mix.state.mute;
      });
    const toggleSolo = (name: string) =>
      update("Solo a track", ({ state }) => {
        state.solo = state.solo.includes(name) ? state.solo.filter((each) => each !== name) : [...state.solo, name];
      });
    const muted = (each: Child) => {
      const mix = mixOf(each);
      return mix?.tool === "fader" && mix.state.mute === true;
    };
    // A text field holds any text: the colour is checked here, a colour of the theme or `#rrggbb`.
    const colourOf = (each: Child): Color => {
      const colour = each.tool === "track" || each.tool === "tape" || each.tool === "bus" ? each.state.colour : undefined;
      const named = PALETTE.find((each) => each.colour === colour)?.colour;
      return named ?? (colour?.startsWith("#") ? (colour as Color) : "blue");
    };
    /** Puts the track `moved` before `before` in `order`. */
    const moveTrack = (moved: string, before: string) =>
      update("Move a track", ({ state }) => {
        const names = rows().map((each) => each.name).filter((name) => name !== moved);
        names.splice(names.indexOf(before), 0, moved);
        state.order = names;
      });
    /** Adds a track of `kind`: 0 an instrument track, 1 an audio track, 2 a return; the menu asks without one. */
    const addTrack = async (kind?: number) => {
      const picked = kind ?? (await openMenu([{ label: "Instrument track" }, { label: "Audio track" }, { label: "Return track" }]));
      if (picked === null) return;
      const names = children.map((child) => child.name);
      const colour = PALETTE[rows().length % PALETTE.length]!.colour;
      update(["Add a track", "Add an audio track", "Add a return"][picked]!, (part) => {
        const sends = part.children.filter((child) => child.tool === "bus").map((back) => ({ name: `send-${back.name}` as const, tool: "fader" as const, state: { gain_db: "-inf" as const } }));
        if (picked === 0) {
          const name = freeName(names, "track");
          part.children.push({
            name, tool: track, state: { colour, title: `Track ${tracks().length + 1}` },
            children: [
              { name: "synth", tool: "instrument.synth", state: {} },
              { name: "chain", tool: chain, state: {} }, { name: "mix", tool: "fader", state: {} }, { name: "rec", tool: "recorder", state: {} },
              ...sends,
            ],
          });
          part.state.order = [...rows().map((each) => each.name), name];
        } else if (picked === 1) {
          const name = freeName(names, "audio-1") as `audio-${string}`;
          part.children.push({ name, tool: tape, state: { colour }, children: [{ name: "rec", tool: "recorder", state: { channels: [0] } }, { name: "chain", tool: chain, state: {} }, { name: "mix", tool: "fader", state: {} }, ...sends] });
          part.state.order = [...rows().map((each) => each.name), name];
        } else {
          const name = [..."abcdefghijklmnopqrstuvwxyz"].map((letter) => `ret-${letter}` as const).find((each) => !names.includes(each)) ?? (freeName(names, "ret-z") as `ret-${string}`);
          part.children.push({ name, tool: bus, state: { title: `Return ${returns().length + 1}`, colour: PALETTE[(returns().length + 3) % PALETTE.length]!.colour }, children: [{ name: "chain", tool: chain, state: {} }, { name: "mix", tool: "fader", state: {} }] });
          for (const each of part.children) {
            if (each.tool === "track" || each.tool === "tape") each.children.push({ name: `send-${name}` as const, tool: "fader", state: { gain_db: "-inf" } });
          }
        }
      });
      if (picked === 2) memory.returnsOpen = true;
    };
    const deleteTrack = (name: string) => {
      update("Delete a track", ({ state, children }) => {
        state.order = state.order.filter((each) => each !== name);
        state.solo = state.solo.filter((each) => each !== name);
        state.sidechains = state.sidechains.filter(({ effect, from }) => from !== name && !effect.startsWith(`${name}/`));
        for (const each of children) {
          if (each.tool === "track") each.children = each.children.filter((child) => child.name !== `send-${name}`);
          if (each.tool === "tape") each.children = each.children.filter((child) => child.name !== `send-${name}`);
        }
        children.splice(0, children.length, ...children.filter((child) => child.name !== name));
      });
      if (memory.track === name) memory.track = null;
      const below = memory.below;
      if ((below?.kind === "track" && below.name === name) || (below?.kind === "editor" && below.clip.startsWith(`${name}/`))) memory.below = null;
      memory.selected = new Set([...memory.selected].filter((key) => !key.startsWith(`${name}/`)));
    };
    const trackMenu = async (each: Child) => {
      const lanesShown = memory.lanes.has(each.name);
      const items = [{ label: "Rename" }, { label: "Colour…" }, { label: lanesShown ? "Hide automation" : "Show automation", disabled: each.tool !== "track" },
        { label: "Add automation…", disabled: each.tool !== "track" }, { label: "Delete" }];
      const picked = await openMenu(items);
      if (picked === 0) memory.renaming = each.name;
      else if (picked === 1) {
        const colour = await openMenu(PALETTE.map(({ name, colour }) => ({ label: name, checked: colourOf(each) === colour })));
        if (colour !== null) {
          update("Colour a track", ({ children }) => {
            const owner = children.find((child) => child.name === each.name);
            if (owner?.tool === "track" || owner?.tool === "tape" || owner?.tool === "bus") owner.state.colour = PALETTE[colour]!.colour;
          });
        }
      } else if (picked === 2) showLanes(each, !lanesShown);
      else if (picked === 3) addLane(each);
      else if (picked === 4) deleteTrack(each.name);
    };

    // Automation
    /** Shows or hides the lanes of `each`, and learns the ranges of what they move. */
    const showLanes = (each: Child, shown: boolean) => {
      const lanes = new Set(memory.lanes);
      if (shown) lanes.add(each.name);
      else lanes.delete(each.name);
      memory.lanes = lanes;
      if (shown) for (const { target } of lanesOf(each)) learnRange(each.name, target);
    };
    const learnRange = async (track: string, target: string) => {
      const [child, number] = splitTarget(target);
      const found = (await numbers(`${track}/${child}`)).find((each) => each.name === number);
      if (found) memory.ranges = { ...memory.ranges, [`${track}/${target}`]: found };
      await learnName(track, child);
    };
    const learnName = async (track: string, path: string) => {
      if (path === "mix") return;
      const { offers: offered, here } = await offers(path === "synth" ? "notes" : "audio", { at: `${track}/${path}` });
      const name = offered.find((each) => each.key === here)?.name;
      if (name) memory.names = { ...memory.names, [`${track}/${path}`]: name };
    };
    /** What a lane of a track can move, a group each, as the arrangement offered them: the track's own volume and pan, its instrument, then its chain's effects in order. */
    const devicesOf = (each: Child) => {
      const effects = each.tool === "track" ? each.children.find((child) => child.name === "chain") : undefined;
      const order = effects?.tool === "chain" ? effects.state.order : [];
      const paths = ["synth", ...order.map((effect) => `chain/${effect}`)];
      const named = (path: string) => memory.names[`${each.name}/${path}`] ?? words(path.split("/").at(-1) ?? path);
      return [["mix", "Track"], ...paths.map((path) => [path, named(path)])] as [string, string][];
    };
    const addLane = async (each: Child) => {
      if (each.tool !== "track") return;
      await Promise.all(devicesOf(each).map(([path]) => learnName(each.name, path)));
      const taken = new Set(each.state.automation.map(({ target }) => target));
      const found = (await Promise.all(devicesOf(each).map(async ([path, group]) => (await numbers(`${each.name}/${path}`)).map((number) => ({ path, group, number })))))
        .flat()
        // The mute is the solo's, as the arrangement offered no lane for it.
        .filter(({ path, number }) => !taken.has(`${path}.${number.name}`) && !(path === "mix" && number.name === "mute"));
      const label = ({ path, number }: (typeof found)[number]) => (path === "mix" && number.name === "gain_db" ? "Volume" : number.label);
      const picked = await openMenu(found.map((each) => ({ label: label(each), group: each.group })), { width: 240, maxHeight: 320, search: true });
      const chosen = picked === null ? undefined : found[picked];
      if (!chosen) return;
      const target = `${chosen.path}.${chosen.number.name}`;
      const value = chosen.number.value ?? valueAt(chosen.number, 0.5);
      update("Add a lane", ({ children }) => {
        const owner = children.find((child) => child.name === each.name);
        if (owner?.tool !== "track") return;
        const lane = freeName(owner.children.map((child) => child.name), `lane-${nameFrom(target)}`) as `lane-${string}`;
        owner.children.push({ name: lane, tool: "lane", state: { points: [{ tick: 0, value }], scale: chosen.number.scale } });
        owner.state.automation = [...owner.state.automation, { lane, target }];
      });
      memory.ranges = { ...memory.ranges, [`${each.name}/${target}`]: chosen.number };
      showLanes(each, true);
    };
    const removeLane = (track: string, lane: string) =>
      update("Remove a lane", ({ children }) => {
        const owner = children.find((child) => child.name === track);
        if (owner?.tool !== "track") return;
        owner.state.automation = owner.state.automation.filter((each) => each.lane !== lane);
        owner.children = owner.children.filter((child) => child.name !== lane);
      });
    /** Changes the points of the lane `lane` of `track`, sorted, one a tick, at least one. */
    const editPoints = (label: string, track: string, lane: string, change: (points: LanePoint[]) => LanePoint[]) =>
      update(label, ({ children }) => {
        const owner = children.find((child) => child.name === track);
        const child = owner?.tool === "track" ? owner.children.find((each) => each.name === lane) : undefined;
        if (child?.tool !== "lane") return;
        const points = [...new Map(change(child.state.points.map((point) => ({ ...point }))).map((point) => [point.tick, point])).values()].sort((a, b) => a.tick - b.tick);
        if (points.length > 0) child.state.points = points;
      });
    const rangeOf = (track: string, target: string, points: readonly LanePoint[]): MovedNumber =>
      memory.ranges[`${track}/${target}`] ?? { name: target, label: words(target), value: null, min: Math.min(0, ...points.map((point) => point.value)), max: Math.max(1, ...points.map((point) => point.value)), scale: "linear" };
    /** A point where a lane was pressed, on the grid of the snap unless cmd is held, chosen. */
    const addPoint = (track: string, { lane, target }: { lane: string; target: string }, x: number, up: number, free: boolean) => {
      const owner = tracks().find((each) => each.name === track);
      const child = owner?.tool === "track" ? owner.children.find((each) => each.name === lane) : undefined;
      const points = child?.tool === "lane" ? child.state.points : [];
      const grid = free || memory.snap <= 0 ? 1 : memory.snap;
      const tick = Math.max(Math.round(x / grid) * grid, 0);
      editPoints("Add a point", track, lane, (all) => [...all, { tick, value: valueAt(rangeOf(track, target, points), up) }]);
      memory.selected = new Set();
      memory.point = { track, lane, tick };
    };

    // Clips' keys
    const timeline = now.space("timeline");
    // The playhead's line covers the point right of where it is, as the window's did.
    const playhead = now.tick.plus(timeline.pointX.times(0.5));
    const handle = timeline.pointX.times(6);

    /** A device the composer picks from what the window offers that takes `takes`, as the window's pickers list them. */
    const pick = async (takes: "notes" | "audio", at?: string) => {
      const { offers: offered, notes, here } = await offers(takes, { at });
      const items = offered.map(({ key, name, detail, group, icon, unavailable }) => ({ label: name, detail: detail ?? undefined, group, icon, checked: key === here, disabled: unavailable !== null }));
      const picked = await openMenu(items, { notes, width: 280, maxHeight: 540 });
      return picked === null ? undefined : offered[picked];
    };

    /**
     * The devices of the track or return `name`: an instrument, which can be changed, or the card
     * of an audio track's chosen region, then the effects of its chain, which can be added, turned
     * off, removed and dragged into another order, and last a track's sends.
     */
    const Rack = ({ name }: { name: string }) => {
      const chainOf = () => {
        const row = strips().find((each) => each.name === name);
        const effects = row?.tool === "track" || row?.tool === "tape" || row?.tool === "bus" ? row.children.find((child) => child.name === "chain") : undefined;
        return effects?.tool === "chain" ? effects : undefined;
      };
      const order = () => chainOf()?.state.order ?? [];
      const isOff = (effect: string) => chainOf()?.state.off.includes(effect) ?? false;
      /** Changes the chain of the track as one undo step `label`. */
      const editChain = (label: string, change: (effects: { state: { order: string[]; off: string[] }; children: { name: string }[] }) => void) =>
        update(label, ({ children }) => {
          const row = children.find((child) => child.name === name);
          const effects = row?.tool === "track" || row?.tool === "tape" || row?.tool === "bus" ? row.children.find((child) => child.name === "chain") : undefined;
          if (effects?.tool === "chain") change(effects);
        });
      const addEffect = async () => {
        const offer = await pick("audio");
        if (!offer) return;
        const taken = chainOf()?.children.map((child) => child.name) ?? [];
        const effect = freeName(taken, nameFrom(offer.key) || "effect");
        addOffer(offer.key, `${name}/chain/${effect}`, "Add an effect");
        editChain("Add an effect", (effects) => (effects.state.order = [...effects.state.order, effect]));
      };
      /** Another device in place of the one at `path`: an instrument from what takes notes, an effect from what takes sound. */
      const replace = async (takes: "notes" | "audio", path: string, label: string) => {
        const offer = await pick(takes, path);
        if (offer) addOffer(offer.key, path, label);
      };
      const remove = (effect: string) =>
        editChain("Remove an effect", (effects) => {
          effects.state.order = effects.state.order.filter((each) => each !== effect);
          effects.state.off = effects.state.off.filter((each) => each !== effect);
          effects.children = effects.children.filter((child) => child.name !== effect);
        });
      const toggle = (effect: string) =>
        editChain(isOff(effect) ? "Turn an effect on" : "Turn an effect off", (effects) => {
          effects.state.off = effects.state.off.includes(effect) ? effects.state.off.filter((each) => each !== effect) : [...effects.state.off, effect];
        });
      // An effect dragged onto another goes before it.
      const moveBefore = (moved: string, before: string) =>
        editChain("Move an effect", (effects) => {
          const rest = effects.state.order.filter((each) => each !== moved);
          rest.splice(rest.indexOf(before), 0, moved);
          effects.state.order = rest;
        });
      const row = () => strips().find((each) => each.name === name);
      const isReturn = () => row()?.tool === "bus";
      return (
        <div style={{ height: RACK, direction: "row", keep: true }}>
          <div style={{ width: HEADER - 1, keep: true, paddingTop: 12 }}>
            <div style={{ height: 32, direction: "row", align: "center", gap: 8, paddingLeft: 20, paddingRight: 8 }}>
              <div style={() => ({ width: 8, height: 8, radius: 4, keep: true, background: row() ? colourOf(row()!) : "blue" })} />
              <div style={{ grow: true, fontSize: 14, weight: "medium", truncate: true, color: "gray-950" }}>{() => (row() ? titleOf(row()!) : name)}</div>
              <Button key="close-panel" icon="x" variant="ghost" size="xs" opacity={0.6} onPress={() => (memory.below = null)} />
            </div>
            <div style={{ direction: "row", paddingLeft: 8, gap: 20 }}>
              <div style={{ align: "center", gap: 8, paddingTop: 5 }}>
                {() => <Volume path={`${name}/mix`} height={PANEL_FADER} input={memory.armed.has(name) ? inputOf(row()) : undefined} />}
                <GainText path={`${name}/mix`} />
              </div>
              <div style={{ align: "start" }}>
                <Knob value={`${name}/mix.pan`} label="Pan" min={-1} max={1} default={0} unit="pan" bipolar />
                <div style={{ direction: "row", gap: 4, paddingTop: 6 }}>
                  <Toggle key={`panel-mute-${name}`} label="M" on={() => (row() ? muted(row()!) : false)} color="peach" onPress={() => toggleMute(name)} />
                  <Show when={() => !isReturn()}>
                    {() => <Toggle key={`panel-solo-${name}`} label="S" on={() => state.solo.includes(name)} color="yellow" onPress={() => toggleSolo(name)} />}
                  </Show>
                </div>
                <Show when={() => row()?.tool === "tape"}>{() => <div style={{ paddingTop: 14 }}><InputSelect name={name} /></div>}</Show>
                <Show when={isReturn}>{() => <div style={{ paddingTop: 14 }}><PreFader name={name} /></div>}</Show>
              </div>
            </div>
          </div>
          <div style={{ width: 1, background: "alpha/5", keep: true }} />
        <div key="rack" style={{ grow: true, direction: "row", gap: 12, paddingLeft: 16, paddingTop: 11, paddingRight: 16, align: "start", scroll: "x" }}>
          {() =>
            row()?.tool === "tape" ? (
              <>
                {() => {
                  const chosen = memory.region;
                  return chosen?.startsWith(`${name}/`) ? <DeviceCard key={`clip-${chosen}`} path={chosen} /> : <EmptyClip />;
                }}
                <div style={{ width: 1, height: CARD, background: "alpha/6", keep: true }} />
              </>
            ) : row()?.tool === "track" ? (
              <DeviceCard path={`${name}/synth`} onTitle={() => replace("notes", `${name}/synth`, "Change the instrument")} />
            ) : null
          }
          <For each={order}>
            {(effect) => (
              <div key={`effect-${effect}`} carry={effect} onDrop={({ carried }) => carried && carried !== effect && moveBefore(carried, effect)}>
                <DeviceCard path={`${name}/chain/${effect}`} on={() => !isOff(effect)} onPower={() => toggle(effect)} onClose={() => remove(effect)}
                  onTitle={() => replace("audio", `${name}/chain/${effect}`, "Change an effect")} />
              </div>
            )}
          </For>
          <div key="add-effect" style={{ direction: "row", align: "center", gap: 4, height: 32, paddingX: 8, radius: 8, hover: "alpha/5", keep: true }} onPress={addEffect}>
            <div style={{ fontSize: 14, weight: "medium", color: "gray-950" }}>Add effect</div>
            <Icon name="chevron-down" size={14} color="gray-700" />
          </div>
          <Show when={() => row()?.tool === "track" && returns().length > 0}>
            {() => (
              <>
                <div style={{ width: 1, height: CARD, background: "alpha/6", keep: true }} />
                <Sends name={name} />
              </>
            )}
          </Show>
        </div>
        </div>
      );
    };

    /** The first card of an audio track while none of its regions is chosen, as a device card looks. */
    const EmptyClip = () => (
      <div key="clip-empty" style={{ width: 88, height: CARD, radius: 10, background: "gray-200", borderColor: "alpha/6", borderWidth: 1, keep: true }}>
        <div style={{ height: 32, direction: "row", align: "center", paddingLeft: 15 }}>
          <div style={{ fontSize: 14, weight: "medium", color: "gray-950" }}>Clip</div>
        </div>
        <div style={{ paddingX: 15, fontSize: 12, lineHeight: 14, color: "gray-800" }}>Select a clip.</div>
      </div>
    );
    /** The channels of the input an audio track records, each alone and then the pair. */
    const INPUTS = [{ label: "In 1", channels: [0] }, { label: "In 2", channels: [1] }, { label: "In 1 + 2", channels: [0, 1] }];
    const InputSelect = ({ name }: { name: string }) => {
      const channels = () => {
        const owner = rows().find((each) => each.name === name);
        const rec = owner?.tool === "tape" ? owner.children.find((child) => child.name === "rec") : undefined;
        return rec?.tool === "recorder" ? rec.state.channels : [];
      };
      const label = () => INPUTS.find((each) => each.channels.join() === channels().join())?.label ?? "In 1";
      const pickInput = async () => {
        const picked = await openMenu(INPUTS.map((each) => ({ label: each.label, group: "Input", checked: each.label === label() })), { width: 160 });
        if (picked === null) return;
        update("Change input", ({ children }) => {
          const owner = children.find((child) => child.name === name);
          const rec = owner?.tool === "tape" ? owner.children.find((child) => child.name === "rec") : undefined;
          if (rec?.tool === "recorder") rec.state.channels = INPUTS[picked]!.channels;
        });
      };
      return (
        <div key={`input-${name}`} style={{ direction: "row", align: "center", gap: 4, height: 24, paddingLeft: 8, paddingRight: 6, radius: 6, background: "alpha/5", hover: "alpha/10" }}
          onPress={pickInput}>
          <div style={{ fontSize: 12, weight: "medium", color: "gray-950" }}>{label}</div>
          <Icon name="chevron-down" size={12} color="gray-700" />
        </div>
      );
    };

    const sendsOf = (each: Child) => (each.tool === "track" || each.tool === "tape" ? each.children.filter((child) => child.name.startsWith("send-")).map((child) => child.name) : []);
    /** A fader as the window drew one: a drag up and down turns `gain_db` of the fader `path`, natively. */
    /** A fader over its meter, as the window drew a strip's volume: two bars, green up to -6 dB and yellow above, with a peak line held 1.5 s, and the handle on them, which a drag up and down turns natively. With `device`, the meter is of what the device plays; with `input`, of those channels of the audio input, as while the track is armed. */
    const Volume = ({ path, height, device, input }: { path: string; height: number; device?: boolean; input?: readonly number[] }) => {
      const place = (amplitude: Live) => amplitude.pow(CURVE).times(ZERO_DB).min(1);
      const hot = Math.pow(Math.pow(10, -6 / 20), CURVE) * ZERO_DB;
      const at = now.value(`${path}.gain_db`, { played: true, fraction: true });
      return (
        <Space key={`volume-${path}`} width={28} height={height} cursor="ns-resize"
          drag={{ y: { value: `${path}.gain_db`, travel: height, default: 0, label: "Change the volume" } }}>
          {([0, 1] as const).flatMap((channel) => {
            const heard = input?.[channel] ?? input?.[0];
            const level = heard !== undefined ? now.input(heard) : device ? now.devicePeak(channel) : now.peak(path, channel);
            const bar = place(level).fall(0.5);
            const peak = place(level).hold(1.5).fall(0.5);
            const x = 8 + channel * 7;
            return [
              <Rect x={x} y={0} width={5} height={height} color="alpha/5" />,
              <Rect x={x} y={bar.min(hot).times(-height).plus(height)} width={5} height={bar.min(hot).times(height)} color="green" />,
              <Rect x={x} y={bar.times(-height).plus(height)} width={5} height={bar.minus(hot).max(0).times(height)} color="yellow" />,
              <Rect x={x} y={peak.times(-height).plus(height)} width={5} height={1} color="gray-950" opacity={peak.gt(0.01)} />,
            ];
          })}
          <Rect x={0} y={at.times(-height).plus(height - 3)} width={28} height={6} radius={3} color="gray-950" />
        </Space>
      );
    };
    /** The gain of the fader `path` as it plays, in dB. */
    const GainText = ({ path }: { path: string }) => (
      <Space key={`gain-${path}`} width={56} height={16}>
        <Label x={28} y={0} text={now.value(`${path}.gain_db`, { played: true })} unit="db" size={12} color="gray-950" align="center" />
      </Space>
    );
    /**
     * A strip of the mixer, as the track panel's: a knob per send, its pan, its volume over its
     * meter, then mute and solo and its name. Every strip keeps the room of the most sends a
     * strip has and of mute and solo, so the faders line up.
     */
    const Strip = ({ each, master }: { each?: Child; master?: true }) => {
      const path = each ? `${each.name}/mix` : "master";
      const sends = () => (each ? sendsOf(each) : []);
      // A knob's name is read once, as it opens.
      const returnTitle = (send: string) =>
        untrack(() => {
          const target = returns().find((one) => `send-${one.name}` === send);
          return target ? titleOf(target) : send.replace(/^send-/, "");
        });
      return (
        <div style={{ width: STRIP, align: "center" }}>
          <For each={sends}>{(send) => <Knob value={`${each!.name}/${send}.gain_db`} label={returnTitle(send)} min={-60} max={6} default={0} unit="db" />}</For>
          <div style={() => ({ height: (mostSends() - sends().length) * KNOB, keep: true })} />
          <Knob value={`${path}.pan`} label="Pan" min={-1} max={1} default={0} unit="pan" bipolar />
          <div style={{ align: "center", gap: 7, paddingTop: 4 }}>
            <Volume path={path} height={MIXER_FADER} device={master} />
            <GainText path={path} />
          </div>
          <div style={{ height: 32, direction: "row", align: "center", gap: 4 }}>
            {each ? <Toggle key={`strip-mute-${each.name}`} label="M" on={() => muted(each)} color="peach" onPress={() => toggleMute(each.name)} /> : null}
            {each && each.tool !== "bus" ? <Toggle key={`strip-solo-${each.name}`} label="S" on={() => state.solo.includes(each.name)} color="yellow" onPress={() => toggleSolo(each.name)} /> : null}
          </div>
          <div key={each ? `strip-name-${each.name}` : "strip-name-master"} style={{ width: STRIP - 8, fontSize: 12, color: "gray-800", align: "center", truncate: true }}
            onPress={() => (each ? openPanel(each.name) : (memory.below = { kind: "master" }))}>
            {() => (each ? titleOf(each) : "Master")}
          </div>
        </div>
      );
    };
    const mostSends = memo(() => Math.max(0, ...rows().map((each) => sendsOf(each).length)));
    /** A strip per track, then on the right the returns' strips and the master's; its limiter is in the master's panel. */
    const Mixer = () => (
      <div key="mixer" style={{ direction: "row", keep: true }}>
        <div style={{ grow: true, direction: "row", gap: 4, paddingLeft: 8, paddingTop: 12, paddingBottom: 8, scroll: "x" }}>
          <For each={rows}>{(each) => <Strip each={each} />}</For>
        </div>
        <div style={{ width: 1, background: "alpha/5", keep: true }} />
        <div style={{ direction: "row", gap: 4, paddingLeft: 8, paddingRight: 8, paddingTop: 12, paddingBottom: 8, keep: true }}>
          <For each={returns}>{(each) => <Strip each={each} />}</For>
        </div>
        <div style={{ width: 1, background: "alpha/5", keep: true }} />
        <div style={{ paddingLeft: 8, paddingRight: 8, paddingTop: 12, paddingBottom: 8, keep: true }}>
          <Strip master />
        </div>
      </div>
    );

    /** A track's sends: a knob for each return, two to a column, after its effects as a send takes the sound after them. */
    const Sends = ({ name }: { name: string }) => {
      const sends = () => {
        const row = rows().find((each) => each.name === name);
        return row ? sendsOf(row) : [];
      };
      const columns = memo(() => Array.from({ length: Math.ceil(sends().length / 2) }, (_, index) => index));
      const returnTitle = (send: string) =>
        untrack(() => {
          const target = returns().find((one) => `send-${one.name}` === send);
          return target ? titleOf(target) : send.replace(/^send-/, "");
        });
      return (
        <div key="sends" style={{ height: CARD, radius: 10, background: "gray-200", borderColor: "alpha/6", borderWidth: 1, paddingX: 15, keep: true }}>
          <div style={{ height: 32, direction: "row", align: "center" }}>
            <div style={{ fontSize: 14, weight: "medium", color: "gray-950" }}>Sends</div>
          </div>
          <div style={{ direction: "row", gap: 8 }}>
            <For each={columns}>
              {(column) => (
                <div>
                  <For each={() => sends().slice(column * 2, column * 2 + 2)}>
                    {(send) => <Knob value={`${name}/${send}.gain_db`} label={returnTitle(send)} min={-60} max={6} default={0} unit="db" />}
                  </For>
                </div>
              )}
            </For>
          </div>
        </div>
      );
    };
    /** Whether the sends into the return `name` take the sound before the tracks' faders. */
    const PreFader = ({ name }: { name: string }) => {
      const on = () => {
        const back = returns().find((each) => each.name === name);
        return back?.tool === "bus" && back.state.pre_fader;
      };
      return (
        <Toggle key={`pre-fader-${name}`} label="Pre fader" on={on}
          onPress={() =>
            update(on() ? "Send after the faders" : "Send before the faders", ({ children }) => {
              const back = children.find((child) => child.name === name);
              if (back?.tool === "bus") back.state.pre_fader = !back.state.pre_fader;
            })
          } />
      );
    };
    // As tall as the master row; four and a half show before they scroll, which says there are more.
    const RETURN_ROW = MASTER_ROW;
    /**
     * The returns, over the master row while the song has any: one row that folds them, with
     * their names when folded and a plus that adds one; open, a row each with its dot, its name
     * and M. A press shows its panel, a right-click its menu.
     */
    const Returns = () => (
      <>
        <div style={{ height: 1, background: "alpha/5", keep: true }} />
        <div style={{ height: 31, direction: "row", align: "center", keep: true }}>
          <div style={{ width: HEADER - 1, height: 31, direction: "row", align: "center", paddingRight: 12, keep: true }}>
            <div key="returns-header" style={{ grow: true, height: 31, direction: "row", align: "center", gap: 5, paddingLeft: 17 }}
              onPress={() => (memory.returnsOpen = !memory.returnsOpen)}>
              <Icon name={() => (memory.returnsOpen ? "chevron-down" : "chevron-right")} size={14} color="gray-700" />
              <div style={{ fontSize: 12, color: "gray-900" }}>Returns</div>
              <div style={{ fontSize: 12, color: "gray-700" }}>{() => String(returns().length)}</div>
            </div>
            <Button key="add-return" icon="plus" variant="ghost" size="xs" onPress={() => addTrack(2)} />
          </div>
          <div style={{ width: 1, height: 31, background: "alpha/5" }} />
          <div key="returns-folded" style={{ grow: true, height: 31, direction: "row", align: "center", gap: 16, paddingLeft: 16 }}
            onPress={() => (memory.returnsOpen = !memory.returnsOpen)}>
            <Show when={() => !memory.returnsOpen}>
              {() => (
                <For each={returns}>
                  {(each) => (
                    <div style={{ direction: "row", align: "center", gap: 6 }}>
                      <div style={() => ({ width: 6, height: 6, radius: 3, keep: true, background: colourOf(each) })} />
                      <div style={{ fontSize: 12, color: "gray-700", truncate: true }}>{() => titleOf(each)}</div>
                    </div>
                  )}
                </For>
              )}
            </Show>
          </div>
        </div>
        <Show when={() => memory.returnsOpen}>
          {() => (
            <div style={() => ({ height: Math.min(returns().length, 4.5) * RETURN_ROW, scroll: "y", keep: true })}>
              <For each={returns}>{(each) => <ReturnRow each={each} />}</For>
            </div>
          )}
        </Show>
      </>
    );
    const ReturnRow = ({ each }: { each: Child }) => (
      <div style={{ height: RETURN_ROW, direction: "row", keep: true }}>
        <div style={{ width: HEADER - 1, height: RETURN_ROW, paddingX: 8, paddingY: 4, keep: true }}>
          <div key={`return-${each.name}`} style={() => ({ grow: true, direction: "row", align: "center", gap: 8, paddingLeft: 12, paddingRight: 4, radius: 6, background: panelOf() === each.name ? "alpha/5" : COLOR.clear })}
            onPress={({ clicks, button }) => (button === "right" ? trackMenu(each) : clicks === 2 ? (memory.renaming = each.name) : openPanel(each.name))}>
            <div style={() => ({ width: 8, height: 8, radius: 4, keep: true, background: muted(each) ? `${colourOf(each)}/40` as Color : colourOf(each) })} />
            {() =>
              memory.renaming === each.name ? (
                <Input value={() => titleOf(each)} width={110} focus onCommit={(title) => rename(each.name, title)} />
              ) : (
                <div style={() => ({ grow: true, fontSize: 14, weight: "medium", truncate: true, color: muted(each) ? "gray-950/40" : "gray-950" })}>{() => titleOf(each)}</div>
              )
            }
            <Toggle key={`mute-${each.name}`} label="M" on={() => muted(each)} color="peach" onPress={() => toggleMute(each.name)} />
          </div>
        </div>
        <div style={{ width: 1, height: RETURN_ROW, background: "alpha/5" }} />
      </div>
    );

    /** The master's panel: its fader and meter, and its limiter. */
    const MasterPanel = () => (
      <div style={{ height: RACK, direction: "row", keep: true }}>
        <div style={{ width: HEADER - 1, keep: true, paddingTop: 12 }}>
          <div style={{ height: 32, direction: "row", align: "center", gap: 8, paddingLeft: 20, paddingRight: 8 }}>
            <div style={{ width: 8, height: 8, radius: 4, borderWidth: 1.5, borderColor: "gray-800", keep: true }} />
            <div style={{ grow: true, fontSize: 14, weight: "medium", color: "gray-950" }}>Master</div>
            <Button key="close-master" icon="x" variant="ghost" size="xs" opacity={0.6} onPress={() => (memory.below = null)} />
          </div>
          <div style={{ direction: "row", paddingLeft: 8 }}>
            <div style={{ align: "center", gap: 7, paddingTop: 5 }}>
              <Volume path="master" height={PANEL_FADER} device />
              <GainText path="master" />
            </div>
          </div>
        </div>
        <div style={{ width: 1, background: "alpha/5", keep: true }} />
        <div style={{ grow: true, direction: "row", gap: 12, paddingLeft: 16, paddingTop: 11, paddingRight: 16, align: "start" }}>
          <DeviceCard path="limit" on={() => !state.limit_off} onPower={() => update(state.limit_off ? "Turn the limiter on" : "Turn the limiter off", (song) => (song.state.limit_off = !song.state.limit_off))} />
        </div>
      </div>
    );

    /** A track's header as the window drew it: its dot and name, and under them its automation, arm, mute and solo. A press anywhere on it but its toggles shows its panel, a double press names it, a right press opens its menu. */
    const TrackHead = ({ each }: { each: Child }) => (
      <div style={() => ({ height: ROW - 8, radius: 6, background: panelOf() === each.name && !memory.region?.startsWith(`${each.name}/`) ? "alpha/5" : COLOR.clear })}
        onPress={({ clicks, button }) => (button === "right" ? trackMenu(each) : clicks === 2 ? (memory.renaming = each.name) : openPanel(each.name))}>
        <div style={{ height: 25, paddingTop: 3, paddingLeft: 12, paddingRight: 9, direction: "row", align: "center", gap: 8 }}>
          <div style={() => ({ width: 8, height: 8, radius: 4, keep: true, background: muted(each) ? `${colourOf(each)}/40` as Color : colourOf(each) })} />
          {() =>
            memory.renaming === each.name ? (
              <Input value={() => titleOf(each)} width={120} focus onCommit={(title) => rename(each.name, title)} />
            ) : (
              <div key={`name-${each.name}`} style={() => ({ grow: true, fontSize: 14, weight: "medium", truncate: true, color: muted(each) ? "gray-900/40" : "gray-900" })}>
                {() => titleOf(each)}
              </div>
            )
          }
          {() => (memory.armed.has(each.name) ? <PeakMeter key={`input-${each.name}`} input={inputOf(each)} horizontal /> : null)}
        </div>
        <div style={{ height: 24, direction: "row", align: "center", paddingLeft: 4, paddingRight: 4, gap: 4 }}>
          <Toggle key={`lanes-${each.name}`} icon="automation" label="Automation" on={() => memory.lanes.has(each.name)}
            count={() => lanesOf(each).length} onPress={() => showLanes(each, !memory.lanes.has(each.name))} />
          <div style={{ grow: true }} />
          <Show when={() => each.tool === "tape"}>
            {() => <Toggle key={`arm-${each.name}`} dot on={() => memory.armed.has(each.name)} color="red" onPress={() => toggleArm(each.name)} />}
          </Show>
          <Toggle key={`mute-${each.name}`} label="M" on={() => muted(each)} color="peach" onPress={() => toggleMute(each.name)} />
          <Toggle key={`solo-${each.name}`} label="S" on={() => state.solo.includes(each.name)} color="yellow" onPress={() => toggleSolo(each.name)} />
        </div>
      </div>
    );
    /** What a lane moves, as a person reads it: `Volume`, `Pan`, or the device and its number, `Filter · Cutoff`. */
    const laneName = (track: string, target: string) => {
      const [path, number] = splitTarget(target);
      const named = memory.ranges[`${track}/${target}`]?.label ?? words(number);
      if (path === "mix") return number === "gain_db" ? "Volume" : named;
      // The device cut short when it is long, so the number shows.
      const device = memory.names[`${track}/${path}`] ?? words(path.split("/").at(-1) ?? path);
      return `${device.length > 14 ? `${device.slice(0, 11).trimEnd()}...` : device} · ${named}`;
    };
    /** A lane's name; right-click it to remove the lane. */
    const LaneHead = ({ track, lane }: { track: string; lane: { lane: string; target: string } }) => (
      <div key={`lane-name-${track}-${lane.lane}`} style={{ grow: true, fontSize: 12, color: "gray-800", truncate: true }}
        onPress={({ button }) => button === "right" && openMenu([{ label: "Remove lane" }]).then((picked) => picked === 0 && removeLane(track, lane.lane))}>
        {() => laneName(track, lane.target)}
      </div>
    );

    /** The points of a lane on row `row`: lines between them, and a handle per point. */
    const LaneRow = ({ track, lane, row, colour }: { track: string; lane: { lane: string; target: string }; row: Accessor<number>; colour: Accessor<Color> }) => {
      const points = () => {
        const owner = tracks().find((each) => each.name === track);
        const child = owner?.tool === "track" ? owner.children.find((each) => each.name === lane.lane) : undefined;
        return child?.tool === "lane" ? child.state.points : [];
      };
      const range = () => rangeOf(track, lane.target, points());
      const yOf = (value: number) => topOf(row()) + 0.1 + (1 - upOf(range(), value)) * 0.8;
      const [pressed, press] = signal<number | null>(null);
      const pointDrag: Drag = {
        id: "point",
        snap: { x: BEAT / 4, grid: true },
        onEnd: ({ dx, dy }) => {
          const moved = points()[pressed() ?? -1];
          if (!moved) return;
          const tick = Math.max(moved.tick + dx, 0);
          editPoints("Move a point", track, lane.lane, (all) =>
            all.map((point, index) => (index === pressed() ? { tick, value: valueAt(range(), upOf(range(), point.value) - dy / 0.8) } : point)),
          );
          memory.point = { track, lane: lane.lane, tick };
          press(null);
        },
      };
      // While a point moves, the lines to it go along.
      const dragged = now.drag("point");
      const at = (index: number): readonly [number | Live, number | Live] => {
        // A row of a point that went is read once more before it goes.
        const point = points()[index] ?? { tick: 0, value: 0 };
        return pressed() === index ? [dragged.dx.plus(point.tick), dragged.dy.plus(yOf(point.value))] : [point.tick, yOf(point.value)];
      };
      const chosen = () => {
        const point = memory.point;
        return point?.track === track && point.lane === lane.lane ? points().findIndex((each) => each.tick === point.tick) : -1;
      };
      /** How the readout of a point shows its value: by the unit its name ends in, or as a share of 0 to 1. */
      const unitOf = () => {
        const [, number] = splitTarget(lane.target);
        const found = (/_(hz|db|ms)$/.exec(number)?.[1] ?? (number === "pan" ? "pan" : undefined)) as "hz" | "db" | "ms" | "pan" | undefined;
        return found ?? (range().min === 0 && range().max === 1 ? "percent" : undefined);
      };
      return (
        <>
          <Rect x={-1e6} y={() => topOf(row())} width={2e6} height={timeline.pointY} color="alpha/5" />
          <For each={points}>
            {(point, index) => {
              const last = () => index() === points().length - 1;
              return [
                // Flat before the first point and after the last.
                <Line from={() => [index() === 0 ? 0 : point.tick, at(index())[1]] as const} to={() => at(index())} color={colour} width={1.5} />,
                <Line from={() => at(index())} to={() => (last() ? ([end(), at(index())[1]] as const) : at(index() + 1))} color={colour} width={1.5} />,
                <Rect x={() => timeline.pointX.times(-3.5).plus(point.tick)} y={() => timeline.pointY.times(-3.5).plus(yOf(point.value))}
                  width={timeline.pointX.times(7)} height={timeline.pointY.times(7)} radius={3.5} color={colour} cursor="grab"
                  drag={pointDrag} anchor={() => ({ x: point.tick })} moves={() => (pressed() === index() ? ["point"] : undefined)}
                  onPress={({ alt }) => {
                    if (alt) return points().length > 1 && editPoints("Remove a point", track, lane.lane, (all) => all.filter((_, each) => each !== index()));
                    press(index());
                    memory.selected = new Set();
                    memory.point = { track, lane: lane.lane, tick: point.tick };
                  }} />,
              ];
            }}
          </For>
          <Show when={() => chosen() >= 0}>
            {() => {
              // By the point as it moves; its readout says what it holds.
              const [x, y] = [() => timeline.pointX.times(0).plus(at(chosen())[0]), () => timeline.pointY.times(0).plus(at(chosen())[1])];
              const value = () => points()[chosen()]?.value ?? 0;
              return [
                <Rect x={() => x().plus(timeline.pointX.times(-5))} y={() => y().plus(timeline.pointY.times(-5))}
                  width={timeline.pointX.times(10)} height={timeline.pointY.times(10)} radius={5} color="gray-950" border={colour} borderWidth={1.5} />,
                <Rect x={() => x().plus(timeline.pointX.times(12))} y={() => y().plus(timeline.pointY.times(-11))} width={timeline.pointX.times(64)} height={timeline.pointY.times(22)}
                  radius={6} color="gray-100" border="alpha/10" borderWidth={1} />,
                <Label x={() => x().plus(timeline.pointX.times(44))} y={() => y().plus(timeline.pointY.times(-7))}
                  text={() => timeline.pointX.times(0).plus(value())} unit={unitOf()} size={12} color="gray-950" align="center" />,
              ];
            }}
          </Show>
        </>
      );
    };

    /** A clip of a track on row `row`: its notes, its title, a handle to resize it. */
    const ClipShapes = ({ each, clip, row, snap }: { each: Child; clip: ReturnType<typeof clipsOf>[number]; row: Accessor<number>; snap: number }) => {
      const key = `${each.name}/${clip.name}`;
      const picked = () => isPicked(key);
      const length = () => lengthOf(clip.state);
      const resizes = () => (picked() ? RESIZES : undefined);
      const startMoves = () => (picked() ? START_MOVES : undefined);
      // Its notes as the window drew them: a line each, between its lowest and highest pitch.
      const miniature = memo(() => {
        const pitches = clip.state.notes.map((note) => note.pitch);
        const [lowest, highest] = [Math.min(...pitches), Math.max(...pitches)];
        const range = highest - lowest + 1;
        const shown = Math.max(range, 12);
        const step = (ROW - 24) / shown;
        const height = clamp(step, 2, 4);
        const bottom = ROW - 12 - ((shown - range) / 2) * step;
        return { lowest, step, height, bottom };
      });
      const dim = () => (muted(each) ? 0.4 : 1);
      // The clip and its notes move together, and its notes end at its end.
      return (
        <Group x={() => clip.state.start} y={() => topOf(row())} clip={() => ({ width: length(), height: 1 })}
          moves={() => (picked() ? MOVES : undefined)} grows={resizes}>
          <Rect x={0} y={4 / ROW} width={length} height={(ROW - 8) / ROW} radius={6} anchor={{ x: 0, y: 0 }}
            color="alpha/5" border={() => (picked() ? "gray-950" : "alpha/10")} borderWidth={1} opacity={dim}
            grows={resizes} moves={startMoves} shrinks={startMoves} drag={moveDrag(snap)} onPress={pressClip(key)} cursor="grab" />
          <For each={() => clip.state.notes}>
            {(note) => (
              <Rect x={() => note.start} y={() => { const m = miniature(); return (m.bottom - (note.pitch - m.lowest + 0.5) * m.step - m.height / 2) / ROW; }}
                width={() => timeline.pointX.negate().plus(note.length).max(timeline.pointX)} height={() => miniature().height / ROW} color={() => colourOf(each)} opacity={dim} />
            )}
          </For>
          <Rect x={() => handle.negate().plus(length())} y={4 / ROW} width={handle} height={(ROW - 8) / ROW} color={COLOR.clear}
            anchor={() => ({ x: length() })} moves={resizes}
            drag={resizeDrag(snap)} onPress={pressClip(key)} cursor="ew-resize" />
          <Rect x={0} y={4 / ROW} width={handle} height={(ROW - 8) / ROW} color={COLOR.clear}
            moves={startMoves} drag={resizeStartDrag(snap)} onPress={pressClip(key)} cursor="ew-resize" />
        </Group>
      );
    };

    /**
     * The handles of a chosen region at its top, 7 points down: a fade at each corner, the gain
     * hollow in the middle, and each fade a line from its bottom corner to its handle. A fade's
     * handle drags across, the gain's up and down, 200 points for its whole range, each with a
     * readout of what it makes.
     */
    const RegionHandles = ({ region, width, plays, edit }: { region: ReturnType<typeof regionsOf>[number]; width: Accessor<number>; plays: Accessor<number>; edit: (label: string, change: (state: FilledStateOfTool<"region">) => void) => void }) => {
      const point = timeline.pointX;
      const [top, bottom] = [(4 + 7) / ROW, (ROW - 4) / ROW];
      const changes = () => clock.tempo_map.tempo_changes;
      const fade = (ms: number) => ticksAcross(changes(), region.state.start, ms / 1000);
      const inside = (x: Live) => x.max(point.times(5)).min(point.times(-5).plus(width()));
      const [fadingIn, fadingOut, gaining] = [now.drag("fade-in"), now.drag("fade-out"), now.drag("gain")];
      const fadeIn = () => inside(fadingIn.dx.plus(fade(region.state.fade_in_ms)));
      const fadeOut = () => inside(fadingOut.dx.plus(width() - fade(region.state.fade_out_ms)));
      // A fade is a time, not a place on the grid: as long as the region plays, less the other fade.
      const fadeMs = (ticks: number, other: number) => clamp(Math.round(secondsBetween(changes(), region.state.start, region.state.start + Math.max(ticks, 0)) * 1000), 0, Math.max(plays() * 1000 - other, 0));
      const fadeDrag = (id: "fade-in" | "fade-out"): Drag => ({
        id,
        axis: "x",
        onEnd: ({ dx }) =>
          id === "fade-in"
            ? edit("Change fade in", (state) => (state.fade_in_ms = fadeMs(fade(state.fade_in_ms) + dx, state.fade_out_ms)))
            : edit("Change fade out", (state) => (state.fade_out_ms = fadeMs(fade(state.fade_out_ms) - dx, state.fade_in_ms))),
      });
      const GAIN_RANGE = [-48, 24] as const;
      const dbPerRow = (ROW * (GAIN_RANGE[1] - GAIN_RANGE[0])) / 200;
      const gainDrag: Drag = {
        id: "gain",
        axis: "y",
        onEnd: ({ dy }) => edit("Change gain", (state) => (state.gain_db = clamp(Math.round((Math.max(state.gain_db, GAIN_RANGE[0]) - dy * dbPerRow) * 10) / 10, GAIN_RANGE[0], GAIN_RANGE[1]))),
      };
      const dot = (x: Accessor<Live> | Live, hollow: boolean, drag: Drag, cursor: "ew-resize" | "ns-resize") => [
        <Circle x={x} y={top} radius={5} color={hollow ? "gray-950" : "gray-50"} drag={drag} cursor={cursor} />,
        <Circle x={x} y={top} radius={3.5} color={hollow ? "gray-50" : "gray-950"} drag={drag} cursor={cursor} />,
      ];
      /** What a handle makes while it is dragged, in a pill right of it. */
      const Readout = ({ x, title, value, unit, held }: { x: Accessor<Live>; title: string; value: Accessor<Live>; unit: "ms" | "db"; held: Live }) => [
        <Rect x={() => x().plus(point.times(10))} y={top - 11 / ROW} width={point.times(116)} height={22 / ROW} radius={6} color="gray-100" border="alpha/10" borderWidth={1} opacity={held} />,
        <Label x={() => x().plus(point.times(18))} y={top - 7 / ROW} text={title} size={12} color="gray-950" opacity={held} />,
        <Label x={() => x().plus(point.times(118))} y={top - 7 / ROW} text={value} unit={unit} size={12} color="gray-950" align="right" opacity={held} />,
      ];
      const msOf = (ticks: Live) => ticks.times(60_000 / BEAT).over(now.bpm).max(0);
      const edge = 4 / ROW;
      return (
        <>
          <Show when={() => region.state.fade_in_ms > 0}>
            {() => <Path points={() => [[0, bottom], [0, edge], [fadeIn(), edge], [fadeIn(), top]]} color="gray-50/50" />}
          </Show>
          <Show when={() => region.state.fade_out_ms > 0}>
            {() => <Path points={() => [[width(), bottom], [width(), edge], [fadeOut(), edge], [fadeOut(), top]]} color="gray-50/50" />}
          </Show>
          <Show when={() => region.state.fade_in_ms > 0}>{() => <Line from={[0, bottom]} to={() => [fadeIn(), top]} color="gray-950" width={1.5} />}</Show>
          <Show when={() => region.state.fade_out_ms > 0}>{() => <Line from={() => [width(), bottom]} to={() => [fadeOut(), top]} color="gray-950" width={1.5} />}</Show>
          {dot(fadeIn, false, fadeDrag("fade-in"), "ew-resize")}
          {dot(fadeOut, false, fadeDrag("fade-out"), "ew-resize")}
          {dot(() => point.times(0).plus(width() / 2), true, gainDrag, "ns-resize")}
          <Readout x={fadeIn} title="Fade in" value={() => msOf(fadingIn.dx.plus(fade(region.state.fade_in_ms)))} unit="ms" held={fadingIn.active} />
          <Readout x={fadeOut} title="Fade out" value={() => msOf(fadingOut.dx.negate().plus(fade(region.state.fade_out_ms)))} unit="ms" held={fadingOut.active} />
          <Readout x={() => point.times(0).plus(width() / 2)} title="Gain" value={() => gaining.dy.times(-dbPerRow).plus(region.state.gain_db).times(10).round().over(10)} unit="db" held={gaining.active} />
        </>
      );
    };
    /** An audio take while it records, as a region looks: over what it lies on, with a red border, its sound so far, from `from` to the playhead. */
    const TakeShapes = ({ each, row, from }: { each: Child; row: Accessor<number>; from: number }) => {
      const width = now.tick.minus(from).max(1);
      return (
        <Group x={from} y={() => topOf(row())}>
          <Rect x={0} y={4 / ROW} width={width} height={(ROW - 8) / ROW} radius={6} color="gray-100" />
          <Rect x={0} y={4 / ROW} width={width} height={(ROW - 8) / ROW} radius={6} color="alpha/5" border="red" borderWidth={1} />
          <Wave x={0} y={(ROW / 2 - 22) / ROW} width={width} height={44 / ROW} list={now.take(`${each.name}/rec`)} band low={-1} high={1} color={() => `${colourOf(each)}/85` as Color} />
        </Group>
      );
    };
    /** A region of an audio track on row `row`: its file's waveform; drag it to move it, an edge to trim it. */
    const RegionShapes = ({ each, region, row }: { each: Child; region: ReturnType<typeof regionsOf>[number]; row: Accessor<number> }) => {
      const key = `${each.name}/${region.name}`;
      const chosen = () => memory.region === key;
      // Seconds of its file, as ticks through the tempo map from where it starts.
      const width = () => ticksAcross(clock.tempo_map.tempo_changes, region.state.start, outputs[`${key}.seconds`] ?? 0);
      const sound = () => now.file(region.state.asset, { from: region.state.file_start_seconds, to: region.state.file_end_seconds });
      // Drawn as it sounds: its gain, and its fades straight from silence at its ends.
      const level = (): [number, number][] => {
        const gain = Math.pow(10, region.state.gain_db / 20);
        const across = width();
        const fade = (ms: number) => Math.min(ticksAcross(clock.tempo_map.tempo_changes, region.state.start, ms / 1000), across);
        const [into, out] = [fade(region.state.fade_in_ms), fade(region.state.fade_out_ms)];
        if (into + out > across && into > 0 && out > 0) {
          const meet = (across * into) / (into + out);
          return [[0, 0], [meet, (gain * meet) / into], [across, 0]];
        }
        return [[0, into > 0 ? 0 : gain], [into, gain], [across - out, gain], [across, out > 0 ? 0 : gain]];
      };
      const editRegion = (label: string, change: (state: FilledStateOfTool<"region">) => void) =>
        update(label, ({ children }) => {
          const owner = children.find((child) => child.name === each.name);
          const draft = owner?.tool === "tape" ? owner.children.find((child) => child.name === region.name) : undefined;
          if (draft?.tool === "region") change(draft.state);
        });
      const moveRegion: Drag = { id: "region", snap: { x: BEAT, grid: true }, onEnd: ({ dx }) => editRegion("Move a region", (state) => (state.start = Math.max(state.start + dx, 0))) };
      const trimRegion: Drag = {
        id: "trim",
        axis: "x",
        snap: { x: BEAT / 4, grid: true },
        onEnd: ({ dx }) =>
          editRegion("Trim a region", (state) => {
            const ticks = Math.max(dx, -state.start);
            const moved = secondsBetween(clock.tempo_map.tempo_changes, state.start, state.start + ticks);
            state.start += ticks;
            state.file_start_seconds = Math.max(state.file_start_seconds + moved, 0);
          }),
      };
      // How long it plays, which the region tells: the end of its file unless it ends before.
      const trimEnd: Drag = {
        id: "end",
        axis: "x",
        snap: { x: BEAT / 4 },
        onEnd: ({ dx }) =>
          editRegion("Trim a region", (state) => {
            const plays = outputs[`${key}.seconds`] ?? 0;
            // Its whole file plays already: there is nothing after its end.
            if (plays <= 0 || (state.file_end_seconds === null && dx >= 0)) return;
            const changes = clock.tempo_map.tempo_changes;
            const end = state.start + ticksAcross(changes, state.start, plays);
            state.file_end_seconds = state.file_start_seconds + Math.max(plays + secondsBetween(changes, end, end + dx), 0.05);
          }),
      };
      const press = ({ button }: Press) => {
        memory.region = key;
        memory.selected = new Set();
        memory.below = { kind: "track", name: each.name };
        if (button === "right") openMenu([{ label: "Delete" }]).then((picked) => picked === 0 && removeRegion(each.name, region.name));
      };
      return (
        <Group x={() => region.state.start} y={() => topOf(row())} moves={() => (chosen() ? ["region"] : undefined)}>
          <Rect x={0} y={4 / ROW} width={width} height={(ROW - 8) / ROW} radius={6} color="alpha/5" border={() => (chosen() ? "gray-950" : "alpha/10")} borderWidth={1}
            drag={moveRegion} onPress={press} cursor="grab" />
          <Wave x={0} y={(ROW / 2 - 22) / ROW} width={width} height={44 / ROW} list={sound} band low={-1} high={1} level={level} color={() => `${colourOf(each)}/85` as Color} />
          <Rect x={0} y={0.06} width={handle} height={0.88} color={COLOR.clear} drag={trimRegion} onPress={press} cursor="ew-resize"
            moves={() => (chosen() ? ["trim"] : undefined)} />
          <Rect x={() => handle.negate().plus(width())} y={0.06} width={handle} height={0.88} color={COLOR.clear} drag={trimEnd} onPress={press} cursor="ew-resize"
            moves={() => (chosen() ? ["end"] : undefined)} />
          <Show when={chosen}>{() => <RegionHandles region={region} width={width} plays={() => outputs[`${key}.seconds`] ?? 0} edit={editRegion} />}</Show>
        </Group>
      );
    };
    const removeRegion = (owner: string, name: string) => {
      memory.region = null;
      update("Remove a region", ({ children }) => {
        const tape = children.find((child) => child.name === owner);
        if (tape?.tool === "tape") tape.children = tape.children.filter((child) => child.name !== name);
      });
    };

    // The ruler: bars, sections, tempo changes and time signatures; under it the repeat range.
    const loop = now.drag("loop");
    // The ruler's label of bar `index`: its number, with the time signature where one starts.
    const barSignatures = memo(() => signaturesOf(runs(), starts().length));
    /** A bar's number, its time signature where it changes, and the name of a section that starts there. */
    const rulerLabel = (index: number): TextPart[] => {
      const here = barSignatures()[index];
      const section = clock.sections.find((each) => each.bar === index + 1);
      const bar = index === 0 || here !== barSignatures()[index - 1] ? `${index + 1}  ${here}` : String(index + 1);
      return section ? [{ text: bar }, { text: `  ${section.name}`, color: "gray-950", weight: "medium" }] : [{ text: bar }];
    };
    const pickSnap = async () => {
      const snaps = snapsOf(4 * BEAT);
      const picked = await openMenu(snaps.map(({ label, ticks }) => ({ label, group: "Snap", checked: ticks === memory.snap })), { width: 160 });
      if (picked !== null) memory.snap = snaps[picked]!.ticks;
    };
    /** A section at the bar line nearest `tick`, named by how many there are, its name open to change. */
    const addSection = (tick: number) => {
      const lines = starts();
      const nearest = lines.reduce((best, start, index) => (Math.abs(start - tick) < Math.abs((lines[best] ?? 0) - tick) ? index : best), 0);
      const bar = nearest + 1;
      if (!clock.sections.some((each) => each.bar === bar)) {
        clock.update("Add a section", (song) => {
          song.sections = [...song.sections, { bar, name: `Section ${song.sections.length + 1}` }].sort((a, b) => a.bar - b.bar);
        });
      }
      memory.editing = { kind: "section", bar };
    };
    const pressRuler = async ({ button, x, clicks }: Press) => {
      const bar = barOf(x);
      if (button !== "right") {
        if (clicks === 2) addSection(x);
        else clock.jump(Math.max(Math.round(x / BEAT) * BEAT, 0));
        return;
      }
      const tick = starts()[bar - 1] ?? 0;
      const section = clock.sections.find((each) => each.bar === bar);
      const tempo = clock.tempo_map.tempo_changes.find((each) => each.tick === tick && tick > 0);
      const picked = await openMenu([
        { label: `Tempo change at bar ${bar}…` }, { label: `Time signature from bar ${bar}…` }, { label: section ? `Rename section "${section.name}"…` : `Section at bar ${bar}…` },
        { label: "Remove the tempo change", disabled: !tempo }, { label: "Remove the section", disabled: !section },
      ], { width: 200 });
      if (picked === 0) memory.editing = { kind: "tempo", bar };
      else if (picked === 1) memory.editing = { kind: "signature", bar };
      else if (picked === 2) memory.editing = { kind: "section", bar };
      else if (picked === 3) clock.update("Remove a tempo change", (song) => (song.tempo_map.tempo_changes = song.tempo_map.tempo_changes.filter((each) => each.tick !== tick)));
      else if (picked === 4) clock.update("Remove a section", (song) => (song.sections = song.sections.filter((each) => each.bar !== bar)));
    };
    const commitEdit = (value: string) => {
      const editing = memory.editing;
      memory.editing = null;
      if (!editing) return;
      const tick = starts()[editing.bar - 1] ?? 0;
      if (editing.kind === "tempo") {
        const bpm = Number(value);
        if (!(bpm > 0)) return;
        clock.update("Change the tempo", (song) => {
          const rest = song.tempo_map.tempo_changes.filter((each) => each.tick !== tick);
          song.tempo_map.tempo_changes = [...rest, { tick, bpm }].sort((a, b) => a.tick - b.tick);
        });
      } else if (editing.kind === "signature") {
        if (!/^\d+\/\d+$/.test(value.trim())) return;
        clock.update("Change the time signature", (song) => {
          const runs = song.tempo_map.time_signatures;
          const count = Math.max(editing.bar, runs.reduce((sum, run) => sum + run.bars, 0)) + 1;
          const each = signaturesOf(runs, count);
          const old = each[editing.bar - 1];
          // From the bar to the next change.
          for (let bar = editing.bar - 1; bar < each.length && each[bar] === old; bar++) each[bar] = value.trim();
          song.tempo_map.time_signatures = runsOf(each);
        });
      } else {
        clock.update("Name a section", (song) => {
          const rest = song.sections.filter((each) => each.bar !== editing.bar);
          song.sections = value.trim() === "" ? rest : [...rest, { bar: editing.bar, name: value.trim() }].sort((a, b) => a.bar - b.bar);
        });
      }
    };
    const editedValue = () => {
      const editing = memory.editing;
      if (!editing) return "";
      const tick = starts()[editing.bar - 1] ?? 0;
      if (editing.kind === "tempo") return String([...clock.tempo_map.tempo_changes].reverse().find((each) => each.tick <= tick)?.bpm ?? 120);
      if (editing.kind === "signature") return signaturesOf(runs(), editing.bar)[editing.bar - 1] ?? "4/4";
      return clock.sections.find((each) => each.bar === editing.bar)?.name ?? "";
    };
    const loopDrag: Drag = {
      id: "loop",
      snap: { x: BEAT, grid: true },
      onEnd: ({ startX, x }) => {
        const [from, to] = [Math.max(Math.round(Math.min(startX, x) / BEAT) * BEAT, 0), Math.round(Math.max(startX, x) / BEAT) * BEAT];
        if (to > from) clock.setRepeat({ from, to });
      },
    };
    // The repeat's bar under the ruler, as the arrangement had it: a click turns it on or off,
    // its body moves it, an edge moves that end, on the beats. Off, it keeps its range.
    const repeat = () => clock.repeat ?? memory.repeatRange;
    const setRepeatRange = (from: number, to: number) => {
      if (to <= from) return;
      if (clock.repeat) clock.setRepeat({ from, to });
      else memory.repeatRange = { from, to };
    };
    const toggleRepeat = () => {
      if (clock.repeat) memory.repeatRange = { ...clock.repeat };
      clock.setRepeat(clock.repeat ? null : { ...memory.repeatRange });
    };
    const repeatDrag = (part: "move" | "start" | "end"): Drag => ({
      id: `repeat-${part}`,
      axis: "x",
      snap: { x: BEAT, grid: true },
      onClick: toggleRepeat,
      onEnd: ({ dx }) => {
        const { from, to } = repeat();
        if (part === "move") {
          const moved = Math.max(from + dx, 0) - from;
          setRepeatRange(from + moved, to + moved);
        } else if (part === "start") setRepeatRange(Math.min(Math.max(from + dx, 0), to - BEAT), to);
        else setRepeatRange(from, Math.max(to + dx, from + BEAT));
      },
    });
    const [repeatMove, repeatStart, repeatEnd] = (["move", "start", "end"] as const).map((part) => repeatDrag(part));
    const [moving, starting, ending] = (["move", "start", "end"] as const).map((part) => now.drag(`repeat-${part}`));

    const openEditor = () => {
      const key = openKey();
      const clip = open();
      if (!key || !clip) return null;
      const [trackName] = key.split("/");
      return (
        <ClipEditor
          clip={() => open()?.state}
          bars={() => starts().map((tick) => tick - (open()?.state.start ?? 0))}
          name={() => {
            const row = rows().find((each) => each.name === trackName);
            return row ? titleOf(row) : trackName ?? "";
          }}
          colour={() => {
            const row = rows().find((each) => each.name === trackName);
            return row ? colourOf(row) : "blue";
          }}
          id={key}
          memory={memory}
          edit={(label, change) => editClips(label, [key], change)}
          preview={(pitch) => play(pitch, { child: trackName as `audio-${string}`, seconds: 0.3 })}
          seek={(tick) => clock.jump((open()?.state.start ?? 0) + tick)}
          openMenu={openMenu}
          onClose={() => (memory.below = { kind: "track", name: trackName ?? "" })}
          height={EDITOR}
        />
      );
    };

    return (
      <div style={{ grow: true, background: COLOR.back }}>
        <TitleBar>
          <Transport clock={clock} record={{ on: () => memory.recording !== null, press: toggleRecording }} end={held} />
        </TitleBar>
        <div>
          <div style={{ direction: "row", gap: 8, align: "center" }}>
            {() => {
              const editing = memory.editing;
              if (!editing) return null;
              const label = { tempo: `Tempo at bar ${editing.bar}`, signature: `Time signature from bar ${editing.bar}`, section: `Section at bar ${editing.bar}` }[editing.kind];
              return (
                <div key="ruler-edit" style={{ direction: "row", gap: 6, align: "center" }}>
                  <div style={{ fontSize: 11, color: COLOR.text }}>{label}</div>
                  <Input key="ruler-edit-field" value={editedValue} width={100} focus onCommit={commitEdit} />
                </div>
              );
            }}
            {() => {
              const key = memory.renamingClip;
              if (!key) return null;
              return (
                <div key="clip-name" style={{ direction: "row", gap: 6, align: "center" }}>
                  <div style={{ fontSize: 11, color: COLOR.text }}>Clip name</div>
                  <Input key="clip-name-field" value={() => clipAt(key)?.state.title || clipAt(key)?.name || ""} width={140} focus
                    onCommit={(title) => {
                      memory.renamingClip = null;
                      editClips("Rename a clip", [key], (clip) => (clip.title = title));
                    }} />
                </div>
              );
            }}
          </div>
        </div>
        <div style={{ direction: "row", grow: true }}>
          <div style={{ width: HEADER - 1, keep: true }}>
            <div style={{ height: RULER, direction: "row", align: "center", paddingLeft: 36, paddingRight: 8 }}>
              <div style={{ grow: true, fontSize: 12, color: "gray-700" }}>Snap</div>
              <div key="song-snap" style={{ direction: "row", align: "center", gap: 4, height: 24, paddingLeft: 8, paddingRight: 6, radius: 6, background: "alpha/5", hover: "alpha/10" }}
                onPress={pickSnap}>
                <div style={{ fontSize: 12, weight: "medium", color: "gray-950" }}>{() => snapsOf(4 * BEAT).find((each) => each.ticks === memory.snap)?.label ?? "Off"}</div>
                <Icon name="chevron-down" size={12} color="gray-700" />
              </div>
            </div>
            <div style={{ grow: true }} follow={{ y: "timeline" }}>
              <For each={layout}>
                {({ each, lane, add }) =>
                  add ? (
                    <div style={{ height: ADD_LANE, paddingX: 8, paddingY: 4 }}>
                      <div key={`add-lane-${each.name}`} style={{ grow: true, direction: "row", align: "center", paddingLeft: 12, paddingRight: 10, radius: 6, background: "alpha/5", hover: "alpha/8" }}
                        onPress={() => addLane(each)}>
                        <div style={{ grow: true, fontSize: 13, weight: "medium", color: "gray-950" }}>Add lane</div>
                        <Icon name="chevron-down" size={14} color="gray-700" />
                      </div>
                    </div>
                  ) : lane ? (
                    <div style={{ height: ROW }}>
                      <div style={{ height: 1, background: "alpha/5", keep: true }} />
                      <div style={{ grow: true, direction: "row", align: "center", paddingLeft: 36, paddingRight: 8 }}>
                        <LaneHead track={each.name} lane={lane} />
                      </div>
                    </div>
                  ) : (
                    <div key={`head-${each.name}`} carry={`track:${each.name}`} onDrop={({ carried }) => carried?.startsWith("track:") && carried.slice(6) !== each.name && moveTrack(carried.slice(6), each.name)}
                      style={{ height: ROW, paddingX: 8, paddingY: 4 }}>
                      <TrackHead each={each} />
                    </div>
                  )
                }
              </For>
              <div style={{ height: ROW, direction: "row", align: "center", paddingX: 8 }}>
                <div key="add-track" style={{ grow: true, direction: "row", align: "center", gap: 5, height: ROW - 8, paddingLeft: 9, paddingRight: 10, radius: 6, hover: "alpha/5" }}
                  onPress={() => addTrack(0)}>
                  <Icon name="plus" size={14} color="gray-800" />
                  <div style={{ fontSize: 12, weight: "medium", color: "gray-800" }}>Add track</div>
                </div>
                <div style={{ width: 1, height: 12, background: "alpha/10" }} />
                <div key="add-track-kind" style={{ width: 24, height: 24, align: "center", justify: "center", radius: 6, hover: "alpha/8" }} onPress={() => addTrack()}>
                  <Icon name="chevron-down" size={14} color="gray-700" />
                </div>
              </div>
            </div>
          </div>
          <div style={{ width: 1, background: "alpha/5", keep: true }} />
          <div style={{ grow: true }}>
            <Space key="ruler" height={RULER - 1} unit={{ x: ACROSS }} extent={() => ({ x: [-LEAD, end()] })} scroll={{ x: -LEAD }} zoom={{ x: [0.1, 8] }} follow={{ x: "timeline" }}
              onPress={pressRuler} drag={loopDrag}>
              <For each={starts}>
                {(tick, bar) => [
                  <Rect x={tick} y={RULER - 8} width={timeline.pointX} height={8} color="alpha/10" />,
                  <Label x={() => tick + 8 / ACROSS} y={8} text={() => rulerLabel(bar())} size={12} color="gray-700" />,
                ]}
              </For>
              <For each={() => clock.tempo_map.tempo_changes.filter((change) => change.tick > 0)}>
                {(change) => <Label x={() => change.tick + 40 / ACROSS} y={8} text={() => `${change.bpm} bpm`} size={12} color="gray-700" />}
              </For>
              <Rect x={() => moving!.dx.plus(starting!.dx).plus(repeat().from)} y={RULER - 10}
                width={() => ending!.dx.minus(starting!.dx).plus(repeat().to - repeat().from)} height={8} radius={2} color={() => (clock.repeat ? "gray-900" : "alpha/15")} />
              <Rect x={() => moving!.dx.plus(repeat().from)} y={RULER - 14} width={() => repeat().to - repeat().from} height={14}
                color={COLOR.clear} cursor="pointer" drag={repeatMove!} />
              <Rect x={() => starting!.dx.plus(repeat().from)} y={RULER - 14} width={handle} height={14}
                color={COLOR.clear} cursor="ew-resize" drag={repeatStart!} />
              <Rect x={() => ending!.dx.plus(repeat().to).minus(handle)} y={RULER - 14} width={handle} height={14}
                anchor={() => ({ x: repeat().to })} color={COLOR.clear} cursor="ew-resize" drag={repeatEnd!} />
              <Rect x={loop.startX.min(loop.x)} y={RULER - 10} width={loop.dx.abs()} height={8} radius={2} color="gray-900" opacity={loop.active} />
              <Line from={[playhead, RULER / 2]} to={[playhead, RULER - 1]} color="gray-950" width={1} />
              <Circle x={playhead} y={RULER / 2} radius={3.5} color="gray-950" />
            </Space>
            <div style={{ height: 1, background: "alpha/5", keep: true }} />
            <Space key="timeline" grow unit={{ x: ACROSS, y: ROW }} extent={() => ({ x: [-LEAD, end()], y: [0, Math.max(tops().at(-1) ?? 0, 1)] })} scroll={{ x: -LEAD }} zoom={{ x: [0.1, 8] }}
              drag={marqueeDrag} onPress={pressTimeline} onDrop={dropOnTimeline}>
              <For each={layout}>
                {({ each, lane, add }, row) =>
                  add ? null : lane ? (
                    <LaneRow track={each.name} lane={lane} row={row} colour={() => colourOf(each)} />
                  ) : (
                    <>
                      <For each={() => regionsOf(each)}>{(region) => <RegionShapes each={each} region={region} row={row} />}</For>
                      {() => {
                        const from = memory.recording;
                        const takes = memory.armed.has(each.name) || each.name === notesTrackOf(children, state.order, memory.track);
                        return from !== null && takes ? <TakeShapes each={each} row={row} from={from} /> : null;
                      }}
                      {() => {
                        const snap = memory.snap;
                        return <For each={() => clipsOf(each)}>{(clip) => <ClipShapes each={each} clip={clip} row={row} snap={snap} />}</For>;
                      }}
                    </>
                  )
                }
              </For>
              <Rect x={marquee.startX.min(marquee.x)} y={marquee.startY.min(marquee.y)} width={marquee.dx.abs()} height={marquee.dy.abs()}
                color="alpha/5" border="alpha/20" borderWidth={1} radius={2} opacity={marquee.active} />
              <Line from={[playhead, 0]} to={[playhead, 1000]} color="gray-950" width={1} />
            </Space>
          </div>
        </div>
        <Show when={() => returns().length > 0}>{() => <Returns />}</Show>
        <div style={{ height: 1, background: "alpha/5", keep: true }} />
        <div style={{ height: MASTER_ROW - 1, direction: "row", align: "center", keep: true }}>
          <div style={{ width: HEADER - 1, height: MASTER_ROW - 1, paddingX: 8, paddingY: 4, keep: true }}>
            <div key="master-row" style={() => ({ grow: true, direction: "row", align: "center", gap: 12, paddingLeft: 16, radius: 6, background: memory.below?.kind === "master" ? "alpha/5" : COLOR.clear })}
              onPress={() => (memory.below = { kind: "master" })}>
              <div style={{ width: 8, height: 8, radius: 4, borderWidth: 1.5, borderColor: "gray-800", keep: true }} />
              <div style={{ fontSize: 14, weight: "medium", color: "gray-900" }}>Master</div>
            </div>
          </div>
          <div style={{ width: 1, height: MASTER_ROW - 1, background: "alpha/5" }} />
          <div style={{ grow: true }} />
          <Button key="mixer-toggle" icon="sliders-horizontal" variant="ghost" size="xs" onPress={() => (memory.below = memory.below?.kind === "mixer" ? null : { kind: "mixer" })} />
          <div style={{ width: 8 }} />
        </div>
        {() => {
          const shown = memory.below;
          const below =
            shown?.kind === "editor" ? openEditor()
            : shown?.kind === "mixer" ? <Mixer />
            : shown?.kind === "master" ? <MasterPanel />
            : shown?.kind === "track" && strips().some((each) => each.name === shown.name) ? <Rack name={shown.name} />
            : null;
          // The hairline under the master row is the top of what opens under it.
          return below === null ? null : (
            <>
              <div style={{ height: 1, background: "alpha/5", keep: true }} />
              {below}
            </>
          );
        }}
      </div>
    );
  },
});

/**
 * A clip whose start moved by `dx`: what it holds stays where it is in the song. The start stops
 * at tick 0, at the first note and `unit` before the end; a lane cut short keeps the value it had
 * at the new start, and the pedal what it was there.
 */
function startMoved(clip: FilledStateOfTool<"timeline">, dx: number, unit: number) {
  const length = lengthOf(clip);
  const by = clamp(dx, -clip.start, Math.min(...clip.notes.map((note) => note.start), length - unit));
  clip.start += by;
  clip.length = length - by;
  for (const note of clip.notes) note.start -= by;
  const pedal = clip.pedal.filter((change) => change.start >= by).map((change) => ({ ...change, start: change.start - by }));
  const held = pedal[0]?.start === 0 ? undefined : clip.pedal.filter((change) => change.start < by).at(-1);
  clip.pedal = held ? [{ ...held, start: 0 }, ...pedal] : pedal;
  for (const lane of ["bend", "mod_wheel", "pressure"] as const) {
    const points = clip[lane];
    const kept = points.filter((point) => point.tick >= by).map((point) => ({ ...point, tick: point.tick - by }));
    const cut = points.some((point) => point.tick < by) && kept[0]?.tick !== 0;
    clip[lane] = cut ? [{ tick: 0, value: Math.round(laneAt(points, by, false)) }, ...kept] : kept;
  }
}

/** Removes the clips `chosen` names, `track/clip`. */
function removeClips({ children }: Part<unknown, typeof parts>, chosen: ReadonlySet<string>) {
  for (const row of children) {
    if (row.tool === "track") row.children = row.children.filter((clip) => clip.tool !== "timeline" || !chosen.has(`${row.name}/${clip.name}`));
  }
}

/** Copies of the clips `chosen` names right after the last of them ends, each on its track: the keys of the copies. */
function duplicateClips({ children }: Part<unknown, typeof parts>, chosen: ReadonlySet<string>): Set<string> {
  const mine = children.flatMap((row) =>
    row.tool === "track" ? row.children.flatMap((clip) => (clip.tool === "timeline" && chosen.has(`${row.name}/${clip.name}`) ? [{ row, clip }] : [])) : [],
  );
  const from = Math.min(...mine.map(({ clip }) => clip.state.start));
  const span = Math.max(...mine.map(({ clip }) => clip.state.start + lengthOf(clip.state))) - from;
  const copies = new Set<string>();
  for (const { row, clip } of mine) {
    const name = freeName(row.children.map((each) => each.name), clip.name);
    row.children = [...row.children, { name, tool: "timeline", state: { ...copyOf(clip.state), start: clip.state.start + span } }];
    copies.add(`${row.name}/${name}`);
  }
  return copies;
}

/** The clips `chosen` names to paste: each with its track's row below the top one's, from the earliest one. */
function copied(tracks: readonly TrackRow[], chosen: ReadonlySet<string>): Clipboard | null {
  const clips = [...chosen].flatMap((key) => {
    const [trackName, clipName] = key.split("/");
    const row = tracks.findIndex((each) => each.name === trackName);
    const clip = tracks[row]?.children.find((child) => child.name === clipName);
    return clip?.tool === "timeline" ? [{ row, track: trackName!, state: copyOf(clip.state) }] : [];
  });
  if (clips.length === 0) return null;
  const top = Math.min(...clips.map((clip) => clip.row));
  return { from: Math.min(...clips.map((clip) => clip.state.start)), clips: clips.map((clip) => ({ ...clip, row: clip.row - top })) };
}

type Rows = readonly { each: { name: string; tool: string } }[];
/**
 * Moves the clips `chosen` names by `dx` ticks and `dy` rows of `layout`, to the track of that
 * row, or copies them there: as one undo step. The keys of where they are after.
 */
function moveClips(update: (label: string, change: (part: Part<unknown, typeof parts>) => void) => void, layout: Rows, chosen: ReadonlySet<string>, dx: number, dy: number, copy: boolean): Set<string> {
  const after = new Set<string>();
  update(copy ? "Copy clips" : "Move clips", ({ children }) => {
    for (const key of chosen) {
      const [trackName, clipName] = key.split("/");
      const from = layout.findIndex(({ each }) => each.name === trackName);
      // Down past lanes and audio tracks to the nearest track.
      let target = layout[clamp(from + dy, 0, layout.length - 1)]?.each.name;
      if (layout.find(({ each }) => each.name === target)?.each.tool !== "track") target = trackName;
      const source = children.find((child) => child.name === trackName);
      const to = children.find((child) => child.name === target);
      if (to?.tool !== "track" || source?.tool !== "track") continue;
      const clip = source.children.find((child) => child.name === clipName);
      if (clip?.tool !== "timeline") continue;
      const start = Math.max(clip.state.start + dx, 0);
      // The automation under it goes along, on its own track: another track may have no such device.
      if (to === source && start !== clip.state.start) {
        const range = { start: clip.state.start, end: clip.state.start + lengthOf(clip.state) };
        for (const { lane } of source.state.automation) {
          const points = source.children.find((child) => child.name === lane);
          if (points?.tool !== "lane") continue;
          const moved = carried(points.state.points, points.state.scale === "log", range, start, copy);
          if (moved) points.state.points = moved;
        }
      }
      if (!copy && to === source) {
        clip.state.start = start;
        after.add(key);
        continue;
      }
      const name = freeName(to.children.map((child) => child.name), clipName!);
      to.children = [...to.children, { name, tool: "timeline", state: { ...clip.state, start } }];
      if (!copy) source.children = source.children.filter((child) => child !== clip);
      after.add(`${to.name}/${name}`);
    }
  });
  return after;
}

type LanePoint = { tick: number; value: number };

/** The value of a lane at `tick`: a straight line between its points on the knob's travel, as it plays. */
function laneAt(points: readonly LanePoint[], tick: number, log: boolean): number {
  const after = points.findIndex((point) => point.tick >= tick);
  if (after <= 0) return (after === 0 ? points[0] : points[points.length - 1])?.value ?? 0;
  const [a, b] = [points[after - 1]!, points[after]!];
  const part = b.tick === a.tick ? 1 : (tick - a.tick) / (b.tick - a.tick);
  return log ? a.value * Math.pow(b.value / a.value, part) : a.value + (b.value - a.value) * part;
}

/**
 * A lane after a clip over `range` moved, or was copied, to start at `to`: the line under it goes
 * along and replaces the line where it lands. The values just outside the place it leaves and the
 * place it lands stay as they were, by a point on each edge, so nothing jumps there. A lane with
 * no point under the clip is not the clip's: `null`, it stays as it is.
 */
function carried(points: readonly LanePoint[], log: boolean, range: { start: number; end: number }, to: number, copy: boolean): LanePoint[] | null {
  const sorted = [...points].sort((a, b) => a.tick - b.tick);
  const { start, end } = range;
  if (!sorted.some(({ tick }) => tick >= start && tick < end)) return null;
  const at = (list: readonly LanePoint[], tick: number) => ({ tick, value: laneAt(list, tick, log) });
  const line = [at(sorted, start), ...sorted.filter(({ tick }) => tick > start && tick < end - 1), at(sorted, end - 1)]
    .map(({ tick, value }) => ({ tick: tick - start + to, value }));
  // A place emptied of its line, with what plays just outside it kept.
  const emptied = (list: LanePoint[], from: number, until: number) => [
    ...list.filter(({ tick }) => tick < from || tick >= until),
    ...(from > 0 ? [at(list, from - 1)] : []),
    at(list, until),
  ];
  const left = copy ? sorted : emptied(sorted, start, end);
  const landed = [...emptied(left.sort((a, b) => a.tick - b.tick), to, to + end - start), ...line];
  // One point a tick; the line it carries wins over an edge.
  const byTick = new Map(landed.map((point) => [point.tick, point]));
  for (const point of line) byTick.set(point.tick, point);
  return [...byTick.values()].sort((a, b) => a.tick - b.tick);
}

/** What the keys and a clip's menu do to the chosen clips. */
type ClipCommand = "cut" | "copy" | "duplicate" | "delete";

/**
 * The track the keyboard plays and records on: the chosen one, or the first in `order` while
 * none is; none while an audio track is chosen.
 */
function notesTrackOf(children: readonly { name: string; tool: string }[], order: readonly string[], chosen: string | null): string | null {
  const rank = (name: string) => (order.includes(name) ? order.indexOf(name) : order.length);
  const tracks = children.filter((child) => child.tool === "track").sort((a, b) => rank(a.name) - rank(b.name) || (a.name < b.name ? -1 : 1));
  if (chosen !== null && children.some((child) => child.name === chosen)) return tracks.some((each) => each.name === chosen) ? chosen : null;
  return tracks[0]?.name ?? null;
}

/**
 * Records from the playhead, or ends the recording: every armed track, and the notes of the
 * keyboard on its track. With nothing armed and an audio track chosen, it arms that one first.
 */
function toggleRecordingOf({ memory, send, clock, children }: Pick<Context<unknown, Inputs, Memory, typeof parts>, "memory" | "send" | "clock" | "children">, order: readonly string[]) {
  const recordable = children.flatMap((each) => ((each.tool === "track" || each.tool === "tape") && each.children.some((child) => child.name === "rec") ? [each.name] : []));
  const record = (name: string, on: boolean) => send({ child: `${name}/rec` as const, number: "record" }, on ? 1 : 0);
  if (memory.recording !== null) {
    for (const name of recordable) record(name, false);
    memory.recording = null;
    return;
  }
  const chosen = children.find((each) => each.name === memory.track);
  if (memory.armed.size === 0 && chosen?.tool === "tape" && recordable.includes(chosen.name)) {
    memory.armed = new Set([chosen.name]);
    send({ child: `${chosen.name}/rec` as const, number: "arm" }, 1);
  }
  const notes = notesTrackOf(children, order, memory.track);
  memory.recording = clock.tick;
  for (const name of new Set([...memory.armed, ...(notes ? [notes] : [])])) if (recordable.includes(name)) record(name, true);
  clock.play();
}

/** What a key does to the chosen clips: copy, cut, paste, duplicate, delete, nudge. `tracks` are in their order. */
function clipKey(key: Key, tracks: readonly TrackRow[], tool: Context<unknown, Inputs, Memory, typeof parts>) {
  const { memory, update, clock } = tool;
  const command: ClipCommand | null = !key.cmd && (key.key === "delete" || key.key === "backspace") ? "delete"
    : !key.cmd ? null : key.key === "x" ? "cut" : key.key === "c" ? "copy" : key.key === "d" ? "duplicate" : null;
  if (command) {
    clipCommand(command, tracks, tool);
  } else if (key.key === "escape") {
    memory.selected = new Set();
  } else if (key.cmd && key.key === "a") {
    memory.selected = new Set(tracks.flatMap((each) => each.children.filter((child) => child.tool === "timeline").map((child) => `${each.name}/${child.name}`)));
  } else if (key.cmd && key.key === "v" && memory.clipboard) {
    // At the playhead: the top one on the chosen track, else on its own, and the others on the rows below as they were.
    const clipboard = memory.clipboard;
    const chosen = tracks.findIndex((each) => each.name === memory.track);
    const pasted = new Set<string>();
    update("Paste clips", ({ children }) => {
      for (const { row, track, state } of clipboard.clips) {
        const name = chosen < 0 ? track : tracks[chosen + row]?.name;
        const owner = children.find((child) => child.name === name);
        if (owner?.tool !== "track") continue;
        const clip = freeName(owner.children.map((child) => child.name), "clip");
        owner.children = [...owner.children, { name: clip, tool: "timeline", state: { ...state, start: Math.round(clock.tick) + (state.start - clipboard.from) } }];
        pasted.add(`${owner.name}/${clip}`);
      }
    });
    memory.selected = pasted;
  } else if (memory.selected.size > 0 && ["left", "right", "up", "down"].includes(key.key) && !key.cmd) {
    const step = memory.snap || BEAT / 4;
    const dx = key.key === "left" ? -step : key.key === "right" ? step : 0;
    const dy = key.key === "up" ? -1 : key.key === "down" ? 1 : 0;
    memory.selected = moveClips(update, tracks.map((each) => ({ each })), memory.selected, dx, dy, false);
  }
}

/** Cut, copy, duplicate or delete the chosen clips, each as one undo step: the copies of a duplicate are chosen after. */
function clipCommand(command: ClipCommand, tracks: readonly TrackRow[], { memory, update, select }: Pick<Context<unknown, Inputs, Memory, typeof parts>, "memory" | "update" | "select">) {
  const chosen = new Set(memory.selected);
  if (chosen.size === 0) return;
  if (command === "cut" || command === "copy") memory.clipboard = copied(tracks, chosen);
  if (command === "duplicate") {
    let copies = new Set<string>();
    update("Duplicate clips", (part) => (copies = duplicateClips(part, chosen)));
    memory.selected = copies;
  } else if (command !== "copy") {
    update(command === "cut" ? "Cut clips" : "Remove clips", (part) => removeClips(part, chosen));
    memory.selected = new Set();
    select(null);
  }
}
