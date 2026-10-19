import { Arc, Button, Circle, DeviceCard, For, TitleBar, Toggle, Icon, Knob, PeakMeter, type Color, Fragment, Group, h, Input, Label, Line, list, memo, now, Rect, selector, Show, signal, Space, text, tool, Transport, Wave, type Accessor, type Connection, type Declaration, type Drag, type Dropped, type FilledStateOfTool, type Frozen, type Context, type Inputs, type Key, type Live, type MovedNumber, type OwnedChild, type Part, type Press } from "./sdk";
import { bus } from "./bus";
import { chain } from "./chain";
import { ClipEditor, editorKey, lengthOf, snapsOf, type EditorMemory, type Lane } from "./clip-editor";
import { freeName, tape } from "./tape";
import { splitTarget, track } from "./track";

// A song: tracks of clips in time, with their automation, a mixer strip per track, the devices
// of the chosen track, and a note editor for one clip.
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
const EDITOR = 352;
// The panel of the chosen track, as tall as a card and the room around it.
const RACK = 192 + 24;
// Points per tick at zoom 1: a bar is 96 points.
const ACROSS = 0.025;
// The ticks left of bar 1, eight points at zoom 1.
const LEAD = 320;
// The pitches a clip's picture spans.
const [LOW, HIGH] = [36, 96];

const COLOR = {
  back: "#121317", lane: "#17191e", bar: "#2a2d35", text: "#7c808c", chosen: "#e9ebef", playhead: "#7ee0a0",
  note: "#00000080", marquee: "#a9b1ff33", off: "#2a2d35", mute: "#f0b44c", solo: "#7ee0a0", meter: "#7ee0a0", clear: "#00000000",
  edit: "#a9b1ff", record: "#ff6b6b", head: "#1c1e24", repeat: "#a9b1ff55", section: "#f0b44c", automation: "#7cc7ff",
} as const;
const PALETTE = [
  { name: "Blue", colour: "blue" }, { name: "Peach", colour: "peach" }, { name: "Mauve", colour: "mauve" }, { name: "Teal", colour: "teal" },
  { name: "Green", colour: "green" }, { name: "Yellow", colour: "yellow" }, { name: "Pink", colour: "pink" }, { name: "Sky", colour: "sky" },
  { name: "Sapphire", colour: "sapphire" }, { name: "Red", colour: "red" }, { name: "Maroon", colour: "maroon" }, { name: "Lavender", colour: "lavender" },
  { name: "Rosewater", colour: "rosewater" }, { name: "Flamingo", colour: "flamingo" },
] as const;

const [MOVES, RESIZES] = [["move"], ["resize"]];
const parts = { master: "fader", limit: "limiter", quiet: "lane", "audio-*": tape, "ret-*": bus, "*": track } as const;
const STRIP = 64;
const FADER = 160;
// The fader of the chosen track's panel.
const PANEL_FADER = 114;

type Clipboard = { from: number; clips: { track: string; state: FilledStateOfTool<"timeline"> }[] };
/** A track of the song as code reads it. */
type TrackRow = Extract<Frozen<OwnedChild<typeof parts>>, { tool: typeof track }>;
/** A copy of a clip's state to change, from a view's record or a key's. */
const copyOf = (state: Frozen<FilledStateOfTool<"timeline">>) => JSON.parse(JSON.stringify(state)) as FilledStateOfTool<"timeline">;
type Editing = { kind: "tempo" | "signature" | "section"; bar: number } | null;

type Memory = {
  /** The chosen clips, `track/clip`, and the chosen region. */
  selected: Set<string>;
  region: string | null;
  open: string | null;
  armed: Set<string>;
  recording: number | null;
  /** The track the keyboard plays, the one whose panel shows under the timeline, and the one whose name is typed. */
  track: string | null;
  panel: string | null;
  renaming: string | null;
  renamingClip: string | null;
  /** What shows under the timeline in place of a track's panel: the master's, or the mixer. */
  master: boolean;
  mixer: boolean;
  clipboard: Clipboard | null;
  /** The tracks whose automation lanes show under them, and each lane's range by `track/target`. */
  lanes: Set<string>;
  ranges: Record<string, MovedNumber>;
  editing: Editing;
  /** The range the ruler shows to repeat, while the clock does not repeat. */
  repeatRange: { from: number; to: number };
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
    "Tracks of clips in time, and the devices of the chosen track under them. Each `track` child and each audio track `audio-*` plays into the fader `master`; `order` is the order of the tracks, `solo` names the tracks that play alone. R arms a track that has a recorder `rec`, and the record button records every armed one from the playhead: notes into new clips, the audio input into new regions of the armed audio tracks. Drag a clip to move it (alt copies it, up and down moves it to another track; on its own track the automation under it goes along), drag its right end to resize it, double-click empty space to add one and a clip to edit its notes below, drag over empty space to choose the clips there; right-click it to rename, copy or delete it. Cmd-C, cmd-X, cmd-V, cmd-D, delete and the arrows work on the chosen clips, or on the chosen notes while the editor has some; cmd-V pastes copied notes while the editor is open. A track's menu (right-click its name) renames it, colours it, shows its automation, adds a lane for a number of its devices, or deletes it; drag a track's head onto another to move it there. A lane's point: double-click to add, drag to move, alt-click to remove. The ruler: click to play from there, right-click to add a tempo change, a time signature or a section; drag under it to repeat a range. A region: drag to move it, an edge to trim it; its gain and fades under the timeline. Space plays and pauses, wherever the focus is. Each of `sidechains` keys an effect, by its path such as `t00/chain/comp`, with what a track `from` plays after its fader.",
  state: { order: list(text(), { max: 128 }), solo: list(text(), { max: 64 }), sidechains: list({ effect: text(), from: text() }, { max: 64 }) },
  children: parts,
  memory: (): Memory => ({
    selected: new Set(), region: null, open: null, armed: new Set(), recording: null, track: null, panel: null, renaming: null, renamingClip: null,
    master: false, mixer: false, clipboard: null, lanes: new Set(), ranges: {}, editing: null, repeatRange: { from: 0, to: 4 * 4 * BEAT },
    notes: new Set(), snap: BEAT / 4, lane: "velocity", length: BEAT / 2, copied: null,
  }),
  keys: ["delete", "backspace", "cmd-a", "cmd-c", "cmd-x", "cmd-v", "cmd-d", "left", "right", "up", "down", "escape"],
  globalKeys: ["space"],
  onKey: (tool, key) => {
    const { state, memory, update, clock, children } = tool;
    const order = (name: string) => (state.order.includes(name) ? state.order.indexOf(name) : state.order.length);
    if (key.key === "space") {
      if (key.down) (clock.playing ? clock.pause() : clock.play());
      return;
    }
    if (!key.down) return;
    // The editor's notes first, while it has some chosen.
    if (memory.open) {
      const [trackName, clipName] = memory.open.split("/");
      const row = children.find((each) => each.name === trackName);
      const clip = row?.tool === "track" ? row.children.find((child) => child.name === clipName) : undefined;
      const key2 = memory.open;
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
    if (memory.region && (key.key === "delete" || key.key === "backspace")) {
      const [owner, name] = memory.region.split("/");
      memory.region = null;
      update("Remove a region", ({ children }) => {
        const tape = children.find((child) => child.name === owner);
        if (tape?.tool === "tape") tape.children = tape.children.filter((child) => child.name !== name);
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
      { from: { child: "master" }, to: { child: "limit" } },
      { from: { child: "limit" }, to: { port: "audio" } },
    ];
    // A solo turns every other track's fader down to silence, gliding; a return always plays.
    for (const each of tracks) {
      if (soloed.length > 0 && !soloed.includes(each.name)) {
        connections.push({ from: { child: "quiet", number: "value" }, to: { child: `${each.name}/mix` as const, number: "gain_db" } });
      }
    }
    // A send of a track plays into its return: after the track's fader, or before it for a return that says so.
    for (const each of tracks) {
      if (each.tool !== "track") continue;
      for (const back of returns) {
        if (back.tool !== "bus" || !each.children.some((child) => child.name === `send-${back.name}`)) continue;
        const from = back.state.pre_fader ? "chain" : "mix";
        const send = `${each.name}/send-${back.name}` as const;
        connections.push({ from: { child: `${each.name}/${from}` as const }, to: { child: send } });
        connections.push({ from: { child: send }, to: { child: back.name } });
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
    const lanesOf = (each: Child) =>
      each.tool === "track"
        ? each.state.automation.flatMap(({ lane, target }) =>
            each.children.some((child) => child.name === lane && child.tool === "lane") ? [{ lane, target }] : [],
          )
        : [];
    type Row = { each: Child; lane: { lane: string; target: string } | null };
    // A row of the timeline for each track, and one for each lane of a track that shows them.
    const layout = memo((): Row[] =>
      rows().flatMap((each) => [{ each, lane: null }, ...(memory.lanes.has(each.name) ? lanesOf(each).map((lane) => ({ each, lane })) : [])]),
    );
    const regionsOf = (each: Child) => (each.tool === "tape" ? each.children.filter((child) => child.tool === "region") : []);
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

    /** Turns the recorders of the armed tracks on or off. */
    const recordArmed = (on: boolean) => {
      // The armed audio tracks, and the chosen instrument track, which the keyboard plays.
      const chosen = tracks().find((each) => each.name === memory.track && recordable(each));
      const recording = new Set([...memory.armed, ...(chosen ? [chosen.name] : [])]);
      for (const name of recording) send({ child: `${name}/rec` as const, number: "record" }, on ? 1 : 0);
    };
    const toggleRecording = () => {
      if (memory.recording === null) {
        memory.recording = clock.tick;
        recordArmed(true);
        clock.play();
      } else {
        recordArmed(false);
        memory.recording = null;
      }
    };
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
    const open = () => (memory.open ? clipAt(memory.open) : undefined);
    /** Chooses the clips `keys` names, `track/clip`: the keyboard plays the track of the first, or `track`, and Export selection exports their bars. */
    const choose = (keys: Set<string>, track: string | null = null) => {
      memory.selected = keys;
      memory.region = null;
      memory.track = ([...keys][0] ?? track)?.split("/")[0] ?? memory.track;
      const clips = [...keys].flatMap((key) => clipAt(key) ?? []);
      const starts = clips.map((clip) => clip.state.start);
      const ends = clips.map((clip) => clip.state.start + lengthOf(clip.state));
      const span = clips.length === 0 ? undefined : { from: Math.min(...starts), to: Math.max(...ends) };
      select([...keys][0] ?? track, { span });
    };
    /** Shows the panel of the track `name` under the timeline, and gives it the keyboard. */
    const openPanel = (name: string) => {
      choose(new Set(), name);
      [memory.panel, memory.master, memory.open] = [name, false, null];
    };
    const openClip = (key: string) => {
      [memory.open, memory.mixer, memory.master] = [key, false, false];
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

    const moveDrag: Drag = {
      id: "move",
      snap: { x: BEAT, y: 1, grid: true },
      onEnd: ({ dx, dy, alt }) => choose(moveClips(update, layout(), memory.selected, dx, Math.round(dy), alt)),
    };
    const marquee = now.drag("clip-marquee");
    const marqueeDrag: Drag = {
      id: "clip-marquee",
      onEnd: ({ startX, startY, x, y, shift }) => {
        const [left, right] = [Math.min(startX, x), Math.max(startX, x)];
        const [top, bottom] = [Math.min(startY, y), Math.max(startY, y)];
        const inside = layout().flatMap(({ each, lane }, row) =>
          lane || row + 1 <= top || row >= bottom
            ? []
            : clipsOf(each)
                .filter(({ state }) => state.start < right && state.start + lengthOf(state) > left)
                .map((clip) => `${each.name}/${clip.name}`),
        );
        choose(new Set([...(shift ? memory.selected : []), ...inside]));
      },
    };
    const resizeDrag: Drag = {
      id: "resize",
      axis: "x",
      snap: { x: BEAT, grid: true },
      onEnd: ({ dx }) =>
        editClips("Resize clips", memory.selected, (clip) => {
          const length = Math.max(lengthOf(clip) + dx, BEAT);
          // What starts at or past its new end is cut: a timeline plays nothing there.
          clip.length = length;
          clip.notes = clip.notes.filter((note) => note.start < length);
          clip.pedal = clip.pedal.filter((change) => change.start < length);
          clip.bend = clip.bend.filter((point) => point.tick < length);
          clip.mod_wheel = clip.mod_wheel.filter((point) => point.tick < length);
          clip.pressure = clip.pressure.filter((point) => point.tick < length);
        }),
    };

    const clipMenu = async (key: string) => {
      if (!memory.selected.has(key)) choose(new Set([key]));
      const chosen = new Set(memory.selected);
      const picked = await openMenu([{ label: "Edit notes" }, { label: "Rename" }, { label: "Copy" }, { label: "Duplicate" }, { label: "Delete" }]);
      if (picked === 0) openClip(key);
      else if (picked === 1) memory.renamingClip = key;
      else if (picked === 2) memory.clipboard = copied(tracks(), chosen);
      else if (picked === 3 || picked === 4) {
        update(picked === 3 ? "Duplicate clips" : "Remove clips", (part) => removeOrCopy(part, chosen, picked === 3));
        if (picked === 4) choose(new Set());
      }
    };
    const pressClip = (key: string) => ({ clicks, shift, button }: Press) => {
      if (button === "right") {
        clipMenu(key);
      } else if (clicks === 2) {
        openClip(key);
      } else if (shift) {
        const next = new Set(memory.selected);
        if (!next.delete(key)) next.add(key);
        choose(next);
      } else if (!memory.selected.has(key)) {
        choose(new Set([key]));
      }
    };
    const pressTimeline = ({ clicks, x, y }: Press) => {
      const row = layout()[Math.floor(y)];
      if (row?.lane) {
        if (clicks === 2) addPoint(row.each.name, row.lane, x, 1 - (y - Math.floor(y) - 0.1) / 0.8);
        return;
      }
      if (clicks !== 2 || row?.each.tool !== "track") {
        choose(new Set());
        return;
      }
      const bar = barOf(x);
      const start = starts()[bar - 1] ?? 0;
      const length = (starts()[bar] ?? start + 4 * BEAT) - start;
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
      const row = layout()[Math.floor(y)]?.each;
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
    const titleOf = (each: Child) => ((each.tool === "track" || each.tool === "bus") && each.state.title) || each.name;
    const rename = (name: string, title: string) => {
      memory.renaming = null;
      update("Rename a track", ({ children }) => {
        const owner = children.find((child) => child.name === name);
        if (owner?.tool === "track" || owner?.tool === "bus") owner.state.title = title;
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
      const colour = each.tool === "track" || each.tool === "tape" ? each.state.colour : undefined;
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
      const picked = kind ?? (await openMenu([{ label: "Instrument track" }, { label: "Audio track" }, { label: "Return" }]));
      if (picked === null) return;
      const names = children.map((child) => child.name);
      const colour = PALETTE[rows().length % PALETTE.length]!.colour;
      update(["Add a track", "Add an audio track", "Add a return"][picked]!, (part) => {
        if (picked === 0) {
          const name = freeName(names, "track");
          const sends = part.children.filter((child) => child.tool === "bus").map((back) => ({ name: `send-${back.name}` as const, tool: "fader" as const, state: { gain_db: "-inf" as const } }));
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
          part.children.push({ name, tool: tape, state: { colour }, children: [{ name: "rec", tool: "recorder", state: { channels: [0] } }, { name: "mix", tool: "fader", state: {} }] });
          part.state.order = [...rows().map((each) => each.name), name];
        } else {
          const name = [..."abcdefghijklmnopqrstuvwxyz"].map((letter) => `ret-${letter}` as const).find((each) => !names.includes(each)) ?? (freeName(names, "ret-z") as `ret-${string}`);
          part.children.push({ name, tool: bus, state: { title: `Return ${returns().length + 1}` }, children: [{ name: "chain", tool: chain, state: {} }, { name: "mix", tool: "fader", state: {} }] });
          for (const each of part.children) {
            if (each.tool === "track") each.children.push({ name: `send-${name}` as const, tool: "fader", state: { gain_db: "-inf" } });
          }
        }
      });
    };
    const deleteTrack = (name: string) => {
      update("Delete a track", ({ state, children }) => {
        state.order = state.order.filter((each) => each !== name);
        state.solo = state.solo.filter((each) => each !== name);
        state.sidechains = state.sidechains.filter(({ effect, from }) => from !== name && !effect.startsWith(`${name}/`));
        for (const each of children) {
          if (each.tool === "track") each.children = each.children.filter((child) => child.name !== `send-${name}`);
        }
        children.splice(0, children.length, ...children.filter((child) => child.name !== name));
      });
      if (memory.track === name) memory.track = null;
      if (memory.panel === name) memory.panel = null;
      memory.selected = new Set([...memory.selected].filter((key) => !key.startsWith(`${name}/`)));
      if (memory.open?.startsWith(`${name}/`)) memory.open = null;
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
            if (owner?.tool === "track" || owner?.tool === "tape") owner.state.colour = PALETTE[colour]!.colour;
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
    };
    /** The devices of a track whose numbers a lane can move: its synth, its fader and its chain's effects. */
    const devicesOf = (each: Child) => {
      const effects = each.tool === "track" ? each.children.find((child) => child.name === "chain") : undefined;
      const order = effects?.tool === "chain" ? effects.state.order : [];
      return [["synth", "Synth"], ["mix", "Fader"], ...order.map((effect) => [`chain/${effect}`, effect])] as [string, string][];
    };
    const addLane = async (each: Child) => {
      if (each.tool !== "track") return;
      const taken = new Set(each.state.automation.map(({ target }) => target));
      const found = (await Promise.all(devicesOf(each).map(async ([path, label]) => (await numbers(`${each.name}/${path}`)).map((number) => ({ path, label, number })))))
        .flat()
        .filter(({ path, number }) => !taken.has(`${path}.${number.name}`));
      const picked = await openMenu(found.map(({ label, number }) => ({ label: `${label} · ${number.name}` })));
      const chosen = picked === null ? undefined : found[picked];
      if (!chosen) return;
      const target = `${chosen.path}.${chosen.number.name}`;
      const value = chosen.number.value ?? valueAt(chosen.number, 0.5);
      update("Add a lane", ({ children }) => {
        const owner = children.find((child) => child.name === each.name);
        if (owner?.tool !== "track") return;
        const lane = freeName(owner.children.map((child) => child.name), `lane-${nameFrom(target)}`) as `lane-${string}`;
        owner.children.push({ name: lane, tool: "lane", state: { points: [{ tick: 0, value }], scale: chosen.number.scale === "log" ? "log" : "linear" } });
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
      memory.ranges[`${track}/${target}`] ?? { name: target, min: Math.min(0, ...points.map((point) => point.value)), max: Math.max(1, ...points.map((point) => point.value)), scale: "linear" };
    const addPoint = (track: string, { lane, target }: { lane: string; target: string }, x: number, up: number) => {
      const owner = tracks().find((each) => each.name === track);
      const child = owner?.tool === "track" ? owner.children.find((each) => each.name === lane) : undefined;
      const points = child?.tool === "lane" ? child.state.points : [];
      const tick = Math.max(Math.round(x / (BEAT / 4)) * (BEAT / 4), 0);
      editPoints("Add a point", track, lane, (all) => [...all, { tick, value: valueAt(rangeOf(track, target, points), up) }]);
    };

    // Clips' keys
    const timeline = now.space("timeline");
    const handle = timeline.pointX.times(6);

    /** The devices of the track `name`: its instrument, which can be changed, its filter and the effects of its chain, which can be added, turned off, removed and dragged into another order. */
    const Rack = ({ name }: { name: string }) => {
      const chainOf = () => {
        const row = tracks().find((each) => each.name === name);
        const effects = row?.tool === "track" ? row.children.find((child) => child.name === "chain") : undefined;
        return effects?.tool === "chain" ? effects : undefined;
      };
      const order = () => chainOf()?.state.order ?? [];
      const isOff = (effect: string) => chainOf()?.state.off.includes(effect) ?? false;
      /** Changes the chain of the track as one undo step `label`. */
      const editChain = (label: string, change: (effects: { state: { order: string[]; off: string[] }; children: { name: string }[] }) => void) =>
        update(label, ({ children }) => {
          const row = children.find((child) => child.name === name);
          const effects = row?.tool === "track" ? row.children.find((child) => child.name === "chain") : undefined;
          if (effects?.tool === "chain") change(effects);
        });
      const addEffect = async () => {
        const offered = await offers("audio");
        const picked = await openMenu(offered.map((offer) => ({ label: offer.name, disabled: offer.unavailable !== null })));
        const offer = picked === null ? undefined : offered[picked];
        if (!offer) return;
        const taken = chainOf()?.children.map((child) => child.name) ?? [];
        const effect = freeName(taken, nameFrom(offer.key) || "effect");
        addOffer(offer.key, `${name}/chain/${effect}`, "Add an effect");
        editChain("Add an effect", (effects) => (effects.state.order = [...effects.state.order, effect]));
      };
      /** Another device in place of the one at `path`: an instrument from what takes notes, an effect from what takes sound. */
      const replace = async (takes: "notes" | "audio", path: string, label: string) => {
        const offered = await offers(takes);
        const picked = await openMenu(offered.map((offer) => ({ label: offer.name, disabled: offer.unavailable !== null })));
        const offer = picked === null ? undefined : offered[picked];
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
      const row = () => rows().find((each) => each.name === name);
      return (
        <div style={{ height: RACK, direction: "row", keep: true }}>
          <div style={{ width: HEADER - 1, keep: true, paddingTop: 13 }}>
            <div style={{ height: 32, direction: "row", align: "center", gap: 8, paddingLeft: 20, paddingRight: 8 }}>
              <div style={() => ({ width: 8, height: 8, radius: 4, keep: true, background: row() ? colourOf(row()!) : "blue" })} />
              <div style={{ grow: true, fontSize: 14, weight: "medium", truncate: true, color: "gray-900" }}>{() => (row() ? titleOf(row()!) : name)}</div>
              <Button key="close-panel" icon="x" variant="ghost" size="xs" onPress={() => (memory.panel = null)} />
            </div>
            <div style={{ direction: "row", paddingLeft: 8, gap: 8 }}>
              <div style={{ align: "center", gap: 6 }}>
                <Volume path={`${name}/mix`} height={PANEL_FADER} />
                <GainText path={`${name}/mix`} />
              </div>
              <div style={{ align: "center", gap: 4 }}>
                <Knob value={`${name}/mix.pan`} label="Pan" min={-1} max={1} default={0} unit="pan" bipolar />
                <div style={{ direction: "row", gap: 4 }}>
                  <Toggle key={`panel-mute-${name}`} label="M" on={() => (row() ? muted(row()!) : false)} color="peach" onPress={() => toggleMute(name)} />
                  <Toggle key={`panel-solo-${name}`} label="S" on={() => state.solo.includes(name)} color="yellow" onPress={() => toggleSolo(name)} />
                </div>
              </div>
            </div>
          </div>
          <div style={{ width: 1, background: "alpha/5", keep: true }} />
        <div key="rack" style={{ grow: true, direction: "row", gap: 12, paddingLeft: 16, paddingTop: 12, paddingRight: 16, align: "start", scroll: "x" }}>
          <DeviceCard path={`${name}/synth`} onTitle={() => replace("notes", `${name}/synth`, "Change the instrument")} />
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
        </div>
        </div>
      );
    };

    /** The chosen region's gain, fades and where in its file it starts. */
    const RegionPanel = ({ path }: { path: string }) => (
      <div key="region-panel" style={{ height: RACK, direction: "row", gap: 12, padding: 12, align: "start" }}>
        <div style={{ fontSize: 11, color: COLOR.text, width: 120 }}>{path}</div>
        <Dial path={`${path}.gain_db`} min={-60} max={6} label="Gain" />
        <Dial path={`${path}.fade_in_ms`} min={0} max={2000} label="Fade in" />
        <Dial path={`${path}.fade_out_ms`} min={0} max={2000} label="Fade out" />
        <Dial path={`${path}.file_start_seconds`} min={0} max={30} label="Offset" travel={600} />
      </div>
    );

    const sendsOf = (each: Child) => (each.tool === "track" ? each.children.filter((child) => child.name.startsWith("send-")).map((child) => child.name) : []);
    /** A fader as the window drew one: a drag up and down turns `gain_db` of the fader `path`, natively. */
    const Fader = ({ path, height = FADER }: { path: string; height?: number }) => {
      const at = now.value(`${path}.gain_db`, { played: true, fraction: true });
      return (
        <Space key={`fader-${path}`} width={28} height={height} cursor="ns-resize"
          drag={{ y: { value: `${path}.gain_db`, travel: height, default: 0, label: "Change the volume" } }}>
          <Rect x={13} y={0} width={2} height={height} radius={1} color="alpha/10" />
          <Rect x={0} y={at.times(-(height - 6)).plus(height - 6)} width={28} height={6} radius={3} color="gray-900" />
        </Space>
      );
    };
    /** A fader over its meter, as the window drew a strip's volume: two bars, green up to -6 dB and yellow above, with a peak line held 1.5 s, and the handle on them, which a drag up and down turns natively. With `device`, the meter is of what the device plays. */
    const Volume = ({ path, height, device }: { path: string; height: number; device?: boolean }) => {
      const place = (amplitude: Live) => amplitude.pow(CURVE).times(ZERO_DB).min(1);
      const hot = Math.pow(Math.pow(10, -6 / 20), CURVE) * ZERO_DB;
      const at = now.value(`${path}.gain_db`, { played: true, fraction: true });
      return (
        <Space key={`volume-${path}`} width={28} height={height} cursor="ns-resize"
          drag={{ y: { value: `${path}.gain_db`, travel: height, default: 0, label: "Change the volume" } }}>
          {([0, 1] as const).flatMap((channel) => {
            const level = device ? now.devicePeak(channel) : now.peak(path, channel);
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
    /** A knob on a number of the fader `path`: its pan, or the gain of a send. */
    const Turn = ({ path, field, label }: { path: string; field: "pan" | "gain_db"; label: string }) => {
      const angle = now.value(`${path}.${field}`, { played: true, fraction: true }).minus(0.5).times(0.75);
      return (
        <Space key={`${field}-${path}`} width={28} height={28} cursor="ns-resize" drag={{ y: { value: `${path}.${field}`, default: 0, label } }}>
          <Arc x={14} y={14} radius={10} from={-0.375} to={0.375} color={COLOR.lane} width={3} />
          <Arc x={14} y={14} radius={10} from={field === "pan" ? 0 : -0.375} to={angle} color={COLOR.edit} width={3} />
        </Space>
      );
    };
    const Strip = ({ each }: { each: Child }) => (
      <div style={{ width: STRIP, gap: 6, align: "center" }}>
        <For each={() => sendsOf(each)}>{(send) => <Turn path={`${each.name}/${send}`} field="gain_db" label="Change a send" />}</For>
        <Turn path={`${each.name}/mix`} field="pan" label="Change the pan" />
        <div style={{ direction: "row", gap: 2 }}>
          <Fader path={`${each.name}/mix`} />
          <PeakMeter of={`${each.name}/mix`} length={FADER} />
        </div>
        <div style={{ direction: "row", gap: 4 }}>
          <Toggle key={`strip-mute-${each.name}`} label="M" on={() => muted(each)} color="peach" onPress={() => toggleMute(each.name)} />
          {each.tool === "bus" ? null : <Toggle key={`strip-solo-${each.name}`} label="S" on={() => state.solo.includes(each.name)} color="yellow" onPress={() => toggleSolo(each.name)} />}
        </div>
        <div style={{ fontSize: 11, color: COLOR.text }}>{() => titleOf(each)}</div>
      </div>
    );
    /** A strip per track and return, the master's fader and its limiter. */
    const Mixer = () => (
      <div key="mixer" style={{ height: 320, direction: "row", gap: 4, padding: 8, align: "end", scroll: "x" }}>
        <For each={() => [...rows(), ...returns()]}>{(each) => <Strip each={each} />}</For>
        <div style={{ width: STRIP, gap: 6, align: "center" }}>
          <div style={{ direction: "row", gap: 2 }}>
            <Fader path="master" />
            <PeakMeter of="master" length={FADER} />
          </div>
          <div style={{ fontSize: 11, color: COLOR.text }}>master</div>
        </div>
        <DeviceCard path="limit" />
      </div>
    );

    /** The master's panel: its fader and meter, and its limiter. */
    const MasterPanel = () => (
      <div style={{ height: RACK, direction: "row", keep: true }}>
        <div style={{ width: HEADER - 1, keep: true, paddingTop: 13 }}>
          <div style={{ height: 32, direction: "row", align: "center", gap: 8, paddingLeft: 18, paddingRight: 8 }}>
            <Icon name="circle" size={12} color="gray-800" />
            <div style={{ grow: true, fontSize: 14, weight: "medium", color: "gray-900" }}>Master</div>
            <Button key="close-master" icon="x" variant="ghost" size="xs" onPress={() => (memory.master = false)} />
          </div>
          <div style={{ paddingLeft: 8, align: "start", gap: 6 }}>
            <Volume path="master" height={PANEL_FADER} device />
            <GainText path="master" />
          </div>
        </div>
        <div style={{ width: 1, background: "alpha/5", keep: true }} />
        <div style={{ grow: true, direction: "row", gap: 12, paddingLeft: 16, paddingTop: 12, paddingRight: 16, align: "start" }}>
          <DeviceCard path="limit" />
        </div>
      </div>
    );

    /** A track's header as the window drew it: its dot and name, and under them its automation, arm, mute and solo. */
    const TrackHead = ({ each }: { each: Child }) => (
      <div style={() => ({ height: ROW - 8, radius: 6, background: memory.panel === each.name ? "alpha/5" : COLOR.clear })}>
        <div style={{ height: 25, paddingTop: 3, paddingLeft: 12, direction: "row", align: "center", gap: 8 }}>
          <div style={() => ({ width: 8, height: 8, radius: 4, keep: true, background: muted(each) ? `${colourOf(each)}/40` as Color : colourOf(each) })} />
          {() =>
            memory.renaming === each.name ? (
              <Input value={() => titleOf(each)} width={120} focus onCommit={(title) => rename(each.name, title)} />
            ) : (
              <div key={`name-${each.name}`} style={() => ({ width: 124, fontSize: 14, weight: "medium", truncate: true, color: muted(each) ? "gray-900/40" : "gray-900" })}
                onPress={({ clicks, button }) =>
                  button === "right" ? trackMenu(each) : clicks === 2 && each.tool === "track" ? (memory.renaming = each.name) : openPanel(each.name)
                }>
                {() => titleOf(each)}
              </div>
            )
          }
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
    const LaneHead = ({ track, lane }: { track: string; lane: { lane: string; target: string } }) => (
      <>
        <div style={{ width: 8 }} />
        <div style={{ grow: true, fontSize: 10, color: COLOR.automation }}>{lane.target.replace(".", " · ")}</div>
        <div key={`remove-${track}-${lane.lane}`} style={{ paddingX: 6, fontSize: 11, color: COLOR.text }} onPress={() => removeLane(track, lane.lane)}>×</div>
      </>
    );

    /** The points of a lane on row `row`: lines between them, and a handle per point. */
    const LaneRow = ({ track, lane, row }: { track: string; lane: { lane: string; target: string }; row: Accessor<number> }) => {
      const points = () => {
        const owner = tracks().find((each) => each.name === track);
        const child = owner?.tool === "track" ? owner.children.find((each) => each.name === lane.lane) : undefined;
        return child?.tool === "lane" ? child.state.points : [];
      };
      const range = () => rangeOf(track, lane.target, points());
      const yOf = (value: number) => row() + 0.1 + (1 - upOf(range(), value)) * 0.8;
      const [pressed, press] = signal<number | null>(null);
      const pointDrag: Drag = {
        id: "point",
        snap: { x: BEAT / 4, grid: true },
        onEnd: ({ dx, dy }) =>
          editPoints("Move a point", track, lane.lane, (all) =>
            all.map((point, index) =>
              index === pressed() ? { tick: Math.max(point.tick + dx, 0), value: valueAt(range(), upOf(range(), point.value) - dy / 0.8) } : point,
            ),
          ),
      };
      return (
        <>
          <Rect x={0} y={row} width={end} height={1} color={COLOR.head} />
          <For each={points}>
            {(point, index) => {
              const next = () => points()[index() + 1];
              return [
                <Line from={() => [index() === 0 ? 0 : point.tick, yOf(point.value)] as const} to={() => [next()?.tick ?? end(), yOf(next()?.value ?? point.value)] as const}
                  color={COLOR.automation} width={2} />,
                <Rect x={() => timeline.pointX.times(-4).plus(point.tick)} y={() => timeline.pointY.times(-4).plus(yOf(point.value))}
                  width={timeline.pointX.times(8)} height={timeline.pointY.times(8)} radius={4} color={COLOR.chosen} cursor="grab"
                  drag={pointDrag} moves={() => (pressed() === index() ? ["point"] : undefined)}
                  onPress={({ alt }) => (alt ? points().length > 1 && editPoints("Remove a point", track, lane.lane, (all) => all.filter((_, each) => each !== index())) : press(index()))} />,
              ];
            }}
          </For>
        </>
      );
    };

    /** A clip of a track on row `row`: its notes, its title, a handle to resize it. */
    const ClipShapes = ({ each, clip, row }: { each: Child; clip: ReturnType<typeof clipsOf>[number]; row: Accessor<number> }) => {
      const key = `${each.name}/${clip.name}`;
      const picked = () => isPicked(key);
      const length = () => lengthOf(clip.state);
      const resizes = () => (picked() ? RESIZES : undefined);
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
        <Group x={() => clip.state.start} y={row} clip={() => ({ width: length(), height: 1 })}
          moves={() => (picked() ? MOVES : undefined)} grows={resizes}>
          <Rect x={0} y={4 / ROW} width={() => timeline.pointX.negate().plus(length())} height={(ROW - 8) / ROW} radius={6} anchor={{ x: 0, y: 0 }}
            color="alpha/5" border={() => (picked() ? "gray-950" : "alpha/10")} borderWidth={1} opacity={dim}
            grows={resizes} drag={moveDrag} onPress={pressClip(key)} cursor="grab" />
          <For each={() => clip.state.notes}>
            {(note) => (
              <Rect x={() => note.start} y={() => { const m = miniature(); return (m.bottom - (note.pitch - m.lowest + 0.5) * m.step - m.height / 2) / ROW; }}
                width={() => timeline.pointX.negate().plus(note.length).max(timeline.pointX)} height={() => miniature().height / ROW} color={() => colourOf(each)} opacity={dim} />
            )}
          </For>
          <Rect x={() => handle.negate().plus(length())} y={4 / ROW} width={handle} height={(ROW - 8) / ROW} color={COLOR.clear}
            anchor={() => ({ x: length() })} moves={resizes}
            drag={resizeDrag} onPress={pressClip(key)} cursor="ew-resize" />
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
        if (button === "right") openMenu([{ label: "Delete" }]).then((picked) => picked === 0 && removeRegion(each.name, region.name));
      };
      return (
        <Group x={() => region.state.start} y={row} moves={() => (chosen() ? ["region"] : undefined)}>
          <Rect x={0} y={4 / ROW} width={width} height={(ROW - 8) / ROW} radius={6} color="alpha/5" border={() => (chosen() ? "gray-950" : "alpha/10")} borderWidth={1}
            drag={moveRegion} onPress={press} cursor="grab" />
          <Wave x={0} y={8 / ROW} width={width} height={(ROW - 16) / ROW} list={sound} band low={-1} high={1} color={() => colourOf(each)} />
          <Rect x={0} y={0.06} width={handle} height={0.88} color={COLOR.clear} drag={trimRegion} onPress={press} cursor="ew-resize"
            moves={() => (chosen() ? ["trim"] : undefined)} />
          <Rect x={() => handle.negate().plus(width())} y={0.06} width={handle} height={0.88} color={COLOR.clear} drag={trimEnd} onPress={press} cursor="ew-resize"
            moves={() => (chosen() ? ["end"] : undefined)} />
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
    const rulerLabel = (index: number) => {
      const here = barSignatures()[index];
      return index === 0 || here !== barSignatures()[index - 1] ? `${index + 1}  ${here}` : String(index + 1);
    };
    const pickSnap = async () => {
      const snaps = snapsOf(4 * BEAT);
      const picked = await openMenu(snaps.map(({ label, ticks }) => ({ label, checked: ticks === memory.snap })));
      if (picked !== null) memory.snap = snaps[picked]!.ticks;
    };
    const pressRuler = async ({ button, x }: Press) => {
      const bar = barOf(x);
      if (button !== "right") {
        clock.jump(Math.max(Math.round(x / BEAT) * BEAT, 0));
        return;
      }
      const tick = starts()[bar - 1] ?? 0;
      const section = clock.sections.find((each) => each.bar === bar);
      const tempo = clock.tempo_map.tempo_changes.find((each) => each.tick === tick && tick > 0);
      const picked = await openMenu([
        { label: `Tempo change at bar ${bar}…` }, { label: `Time signature from bar ${bar}…` }, { label: section ? `Rename section "${section.name}"…` : `Section at bar ${bar}…` },
        { label: "Remove the tempo change", disabled: !tempo }, { label: "Remove the section", disabled: !section },
      ]);
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

    const openEditor = () => {
      const key = memory.open;
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
          openMenu={openMenu}
          onClose={() => (memory.open = null)}
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
                {({ each, lane }) =>
                  lane ? (
                    <div style={{ height: ROW, direction: "row", gap: 6, align: "center", paddingLeft: 36, paddingRight: 8 }}>
                      <LaneHead track={each.name} lane={lane} />
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
            <Space key="ruler" height={RULER} unit={{ x: ACROSS }} extent={() => ({ x: [-LEAD, end()] })} scroll={{ x: -LEAD }} zoom={{ x: [0.1, 8] }} follow={{ x: "timeline" }}
              onPress={pressRuler} drag={loopDrag}>
              <For each={starts}>
                {(tick, bar) => [
                  <Rect x={tick} y={RULER - 8} width={timeline.pointX} height={8} color="alpha/10" />,
                  <Label x={() => tick + 8 / ACROSS} y={8} text={() => rulerLabel(bar())} size={12} color="gray-700" />,
                ]}
              </For>
              <For each={() => clock.sections}>
                {(section) => <Label x={() => (starts()[section.bar - 1] ?? 0) + 8 / ACROSS} y={-4} text={() => section.name} size={10} color="yellow" />}
              </For>
              <For each={() => clock.tempo_map.tempo_changes.filter((change) => change.tick > 0)}>
                {(change) => <Label x={() => change.tick + 40 / ACROSS} y={8} text={() => `${change.bpm} bpm`} size={12} color="gray-700" />}
              </For>
              {() => {
                const range = clock.repeat ?? memory.repeatRange;
                return <Rect x={range.from} y={RULER - 10} width={range.to - range.from} height={8} radius={2} color={clock.repeat ? "gray-900" : "alpha/15"} />;
              }}
              <Rect x={loop.startX.min(loop.x)} y={RULER - 10} width={loop.dx.abs()} height={8} radius={2} color="gray-900" opacity={loop.active} />
              <Line from={[now.tick, RULER / 2]} to={[now.tick, RULER]} color="gray-950" width={1} />
              <Circle x={now.tick} y={RULER / 2} radius={3.5} color="gray-950" />
            </Space>
            <div style={{ height: 1, background: "alpha/5", keep: true }} />
            <Space key="timeline" grow unit={{ x: ACROSS, y: ROW }} extent={() => ({ x: [-LEAD, end()], y: [0, Math.max(layout().length, 1)] })} scroll={{ x: -LEAD }} zoom={{ x: [0.1, 8] }}
              drag={marqueeDrag} onPress={pressTimeline} onDrop={dropOnTimeline}>
              <For each={layout}>
                {({ each, lane }, row) =>
                  lane ? (
                    <LaneRow track={each.name} lane={lane} row={row} />
                  ) : (
                    <>
                      <For each={() => regionsOf(each)}>{(region) => <RegionShapes each={each} region={region} row={row} />}</For>
                      <Show when={() => memory.recording !== null && memory.armed.has(each.name) && each.tool === "tape"}>
                        {() => (
                          <Wave x={() => memory.recording ?? 0} y={() => row() + 0.12} width={() => now.tick.minus(memory.recording ?? 0).max(1)}
                            height={0.76} list={now.take(`${each.name}/rec`)} band low={-1} high={1} color="red" />
                        )}
                      </Show>
                      <For each={() => clipsOf(each)}>{(clip) => <ClipShapes each={each} clip={clip} row={row} />}</For>
                    </>
                  )
                }
              </For>
              <Rect x={marquee.startX.min(marquee.x)} y={marquee.startY.min(marquee.y)} width={marquee.dx.abs()} height={marquee.dy.abs()}
                color="alpha/5" border="alpha/20" borderWidth={1} radius={2} opacity={marquee.active} />
              <Line from={[now.tick, 0]} to={[now.tick, 1000]} color="gray-950" width={1} />
            </Space>
          </div>
        </div>
        <div style={{ height: 1, background: "alpha/5", keep: true }} />
        <div style={{ height: MASTER_ROW - 1, direction: "row", align: "center", keep: true }}>
          <div style={{ width: HEADER - 1, height: MASTER_ROW - 1, paddingX: 8, paddingY: 4, keep: true }}>
            <div key="master-row" style={() => ({ grow: true, direction: "row", align: "center", gap: 8, paddingLeft: 10, radius: 6, background: memory.master ? "alpha/5" : COLOR.clear })}
              onPress={() => ([memory.master, memory.panel, memory.mixer, memory.open] = [true, null, false, null])}>
              <Icon name="circle" size={12} color="gray-800" />
              <div style={{ fontSize: 14, weight: "medium", color: "gray-900" }}>Master</div>
            </div>
          </div>
          <div style={{ width: 1, height: MASTER_ROW - 1, background: "alpha/5" }} />
          <div style={{ grow: true }} />
          <Button key="mixer-toggle" label="Mixer" variant="ghost" size="xs" onPress={() => ([memory.mixer, memory.master, memory.panel, memory.open] = [!memory.mixer, false, null, null])} />
          <div style={{ width: 8 }} />
        </div>
        <div style={{ height: 1, background: "alpha/5", keep: true }} />
        {() => {
          if (memory.open) return openEditor();
          if (memory.mixer) return <Mixer />;
          if (memory.master) return <MasterPanel />;
          const region = memory.region;
          if (region) return <RegionPanel path={region} />;
          const shown = memory.panel;
          return shown !== null && tracks().some((each) => each.name === shown) ? <Rack name={shown} /> : null;
        }}
      </div>
    );
  },
});

/** Removes the clips `chosen` names, `track/clip`, or copies each on after itself. */
function removeOrCopy({ children }: Part<unknown, typeof parts>, chosen: ReadonlySet<string>, copy: boolean) {
  for (const row of children) {
    if (row.tool !== "track") continue;
    const mine = row.children.filter((clip) => clip.tool === "timeline" && chosen.has(`${row.name}/${clip.name}`));
    if (!copy) {
      row.children = row.children.filter((clip) => !mine.includes(clip));
      continue;
    }
    const copies = mine.flatMap((clip) => {
      if (clip.tool !== "timeline") return [];
      const length = lengthOf(clip.state);
      const name = freeName(row.children.map((each) => each.name), clip.name);
      return [{ name, tool: "timeline" as const, state: { ...clip.state, start: clip.state.start + length } }];
    });
    row.children = [...row.children, ...copies];
  }
}

/** The clips `chosen` names, from the earliest one, to paste. */
function copied(tracks: readonly TrackRow[], chosen: ReadonlySet<string>): Clipboard | null {
  const clips = [...chosen].flatMap((key) => {
    const [trackName, clipName] = key.split("/");
    const clip = tracks.find((each) => each.name === trackName)?.children.find((child) => child.name === clipName);
    return clip?.tool === "timeline" ? [{ track: trackName!, state: copyOf(clip.state) }] : [];
  });
  if (clips.length === 0) return null;
  return { from: Math.min(...clips.map((clip) => clip.state.start)), clips };
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

/** What a key does to the chosen clips: copy, cut, paste, duplicate, delete, nudge. `tracks` are in their order. */
function clipKey(key: Key, tracks: readonly TrackRow[], { memory, update, clock, select }: Context<unknown, Inputs, Memory, typeof parts>) {
  const chosen = new Set(memory.selected);
  if (key.key === "escape") {
    memory.selected = new Set();
  } else if (key.cmd && key.key === "a") {
    memory.selected = new Set(tracks.flatMap((each) => each.children.filter((child) => child.tool === "timeline").map((child) => `${each.name}/${child.name}`)));
  } else if (key.cmd && key.key === "v" && memory.clipboard) {
    // At the playhead, on the chosen track or else where they came from.
    const clipboard = memory.clipboard;
    const at = Math.round(clock.tick / BEAT) * BEAT;
    const pasted = new Set<string>();
    update("Paste clips", ({ children }) => {
      for (const { track, state } of clipboard.clips) {
        const owner = children.find((child) => child.name === memory.track && child.tool === "track") ?? children.find((child) => child.name === track);
        if (owner?.tool !== "track") continue;
        const name = freeName(owner.children.map((child) => child.name), "clip");
        owner.children = [...owner.children, { name, tool: "timeline", state: { ...state, start: at + (state.start - clipboard.from) } }];
        pasted.add(`${owner.name}/${name}`);
      }
    });
    memory.selected = pasted;
  } else if (chosen.size === 0) {
    return;
  } else if (key.cmd && key.key === "c") {
    memory.clipboard = copied(tracks, chosen);
  } else if (key.key === "delete" || key.key === "backspace" || (key.cmd && key.key === "x")) {
    if (key.cmd) memory.clipboard = copied(tracks, chosen);
    update(key.cmd ? "Cut clips" : "Remove clips", (part) => removeOrCopy(part, chosen, false));
    memory.selected = new Set();
    select(null);
  } else if (key.cmd && key.key === "d") {
    update("Duplicate clips", (part) => removeOrCopy(part, chosen, true));
  } else if (["left", "right", "up", "down"].includes(key.key)) {
    const dx = key.key === "left" ? -BEAT : key.key === "right" ? BEAT : 0;
    const dy = key.key === "up" ? -1 : key.key === "down" ? 1 : 0;
    memory.selected = moveClips(update, tracks.map((each) => ({ each })), chosen, dx, dy, false);
  }
}
