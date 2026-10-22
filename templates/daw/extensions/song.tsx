import { boolean, choice, Fragment, h, list, memo, Show, text, TitleBar, tool, untrack, type Connection, type Context, type Declaration, type Inputs, type MovedNumber, type ValuesOf } from "./sdk";
import { bus } from "./bus";
import { ClipEditor, lengthOf, type EditorMemory } from "./clip-editor";
import type { Clipboard } from "./song-clips";
import { songKey } from "./song-keys";
import { Mixer } from "./song-mixer";
import { BEAT, clipsOf, colourOf, COLOR, pageOf, regionsOf, titleOf } from "./song-page";
import { MasterPanel, Rack } from "./song-panel";
import { Ruler, SectionName, type Mark } from "./song-ruler";
import { ClipName, Timeline } from "./song-timeline";
import { Heads, MasterRow, notesTrackOf, Returns, toggleRecordingOf } from "./song-tracks";
import { tape } from "./tape";
import { track } from "./track";
import { Transport } from "./transport";

// A song: tracks of clips in time, with their automation, a mixer strip per track, the devices
// of the chosen track, and a note editor for one clip.
export const parts = { master: "fader", limit: "limiter", quiet: "lane", "audio-*": tape, "ret-*": bus, "*": track } as const;
const fields = { order: list(text(), { max: 128 }), solo: list(text(), { max: 64 }), sidechains: list({ effect: text(), from: text(), tap: choice({ options: ["pre_fx", "post_fx", "post_fader"], default: "post_fader" }) }, { max: 64 }), limit_off: boolean({ default: false }) };
/** What the song's code gets: its view, the parts of its page and `onKey`. */
export type Song = Context<ValuesOf<typeof fields>, Inputs, Memory, typeof parts>;
const EDITOR = 352;

/** A section whose name is being typed: its bar, and the tick it starts at. */
type Editing = { bar: number; tick: number } | null;
/** What shows under the timeline, one at a time: a track's panel, the master's, a clip's notes or the mixer. */
type Below = { kind: "track"; name: string } | { kind: "master" } | { kind: "editor"; clip: string } | { kind: "mixer" } | null;

type Memory = {
  /** The chosen clips and regions, `row/name`. */
  selected: Set<string>;
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
  /** The chosen mark of the ruler, which delete removes. */
  mark: Mark | null;
} & EditorMemory;

export const song = tool({
  name: "song",
  title: "Song",
  when: "You change the song page: its tracks, clips, automation, mixer strips or note editor",
  doc:
    "Tracks of clips in time, and the devices of the chosen track under them. Each `track` child and each audio track `audio-*` plays into the fader `master`, then its limiter `limit` unless `limit_off`; `order` is the order of the tracks, `solo` names the tracks that play alone. A track's R arms its recorder `rec`. The key R or the record button records from the playhead until pressed again or a stop: every armed track, the audio input into new regions, and the keyboard's notes into a new clip of the chosen track, or of the first while none is chosen, which the keyboard plays then. Drags, arrows and new clips land on the snap in the corner; cmd held drags off it. Clips and regions are one selection. Drag one to move the chosen (alt copies them, up and down to another track of their kind; the automation under them goes along, a device's lane on its own track only), drag either end of a clip to resize it, double-click empty space to add one and a clip (or enter) to edit its notes below, drag over empty space to choose what is there, shift- or cmd-click to add one; right-click it to cut, copy, duplicate, rename, render or delete it. Cmd-C, cmd-X, cmd-V, cmd-D, delete and the arrows work on the chosen clips and regions, cut and paste with the automation under them, or on the chosen notes while the editor has some; cmd-V pastes copied notes while the editor is open, alt-up and alt-down step the chosen velocities, and escape closes it. A track's menu (right-click its name) renames it, colours it, shows its automation, adds a lane for a number of its devices, renders it, or deletes it; a render (Render to audio track, with or without the tail) plays the track alone, after its effects, from its first chosen clip to its last (all its clips from a track's menu), onto a new audio track `audio-*` under it with its fader and sends, and mutes the track; drag a track's head onto another to move it there; delete on a track whose panel shows asks, then deletes it. A lane: click to add a point, chosen, drag one to move it, delete removes the chosen one and alt-click any. The returns sit over the master row, each with a panel of its effects; a track's panel ends with a knob per send. The ruler: click to play from there, click a section, time signature or tempo change to choose it (delete removes it, enter names a section), double-click to add a section, right-click to add a tempo change or a time signature, which the transport then changes, or a section, T adds a tempo change at the playhead, drag over it to repeat a range; click the repeat's bar at its foot to turn it on or off, drag it or an end to move it. A region chosen alone: an edge trims it, its handles change its gain and fades, alt-up and alt-down its gain. Space plays and pauses, wherever the focus is. Each of `sidechains` keys an effect, by its path such as `t00/chain/comp`, with what a track `from` plays at `tap`: into its effects, out of them or out of its fader; a compressor's or a gate's card picks it behind its expand.",
  state: fields,
  children: parts,
  memory: (): Memory => ({
    selected: new Set(), below: null, armed: new Set(), recording: null, track: null, renaming: null, renamingClip: null,
    clipboard: null, lanes: new Set(), ranges: {}, editing: null, repeatRange: { from: 0, to: 4 * 4 * BEAT }, returnsOpen: false, point: null, mark: null, names: {},
    notes: new Set(), snap: BEAT / 4, lane: "velocity", length: BEAT / 2, copied: null,
  }),
  keys: ["delete", "backspace", "cmd-a", "cmd-c", "cmd-x", "cmd-v", "cmd-d", "left", "right", "up", "down", "alt-up", "alt-down", "escape", "r", "enter", "cmd-down", "t"],
  globalKeys: ["space"],
  onKey: songKey,
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
    // A sidechain keys an effect with a track: what plays into its effects, what comes out of them, or what comes out of its fader.
    for (const { effect, from, tap } of state.sidechains) {
      const source = tracks.find((each) => each.name === from);
      if (!source) continue;
      const before = source.children.filter((child) => child.tool === "region" || child.name === "synth").map((child) => child.name);
      for (const key of tap === "pre_fx" ? before : [tap === "post_fx" ? "chain" : "mix"]) {
        connections.push({ from: { child: `${from}/${key}` as `${string}/${string}` }, to: { child: effect as `${string}/${string}`, port: "sidechain" } });
      }
    }
    return { connections };
  },
  view: (tool) => {
    const { state, memory, clock, play } = tool;
    const page = pageOf(tool);
    const { rows, returns, strips, starts, clipAt, editClips } = page;
    const toggleRecording = () => toggleRecordingOf(tool, state.order);
    // A pause or a stop ends the recording, as in the arrangement.
    memo(() => {
      if (!clock.playing && untrack(() => memory.recording) !== null) untrack(toggleRecording);
    });
    // Where what the song holds ends: the strip of the transport goes this far.
    const held = memo(() => {
      const clips = rows().flatMap((each) => clipsOf(each).map((clip) => clip.state.start + lengthOf(clip.state)));
      const regions = rows().flatMap((each) => regionsOf(each).map((region) => region.state.start));
      return Math.max(0, ...clips, ...regions);
    });

    const openKey = () => (memory.below?.kind === "editor" ? memory.below.clip : null);
    const open = () => {
      const key = openKey();
      return key ? clipAt(key) : undefined;
    };
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
            <SectionName page={page} />
            <ClipName page={page} />
          </div>
        </div>
        <div style={{ direction: "row", grow: true }}>
          <Heads page={page} />
          <div style={{ width: 1, background: "alpha/5", keep: true }} />
          <div style={{ grow: true }}>
            <Ruler page={page} />
            <div style={{ height: 1, background: "alpha/5", keep: true }} />
            <Timeline page={page} />
          </div>
        </div>
        <Show when={() => returns().length > 0}>{() => <Returns page={page} />}</Show>
        <div style={{ height: 1, background: "alpha/5", keep: true }} />
        <MasterRow page={page} />
        {() => {
          const shown = memory.below;
          const below =
            shown?.kind === "editor" ? openEditor()
            : shown?.kind === "mixer" ? <Mixer page={page} />
            : shown?.kind === "master" ? <MasterPanel page={page} />
            : shown?.kind === "track" && strips().some((each) => each.name === shown.name) ? <Rack page={page} name={shown.name} />
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
