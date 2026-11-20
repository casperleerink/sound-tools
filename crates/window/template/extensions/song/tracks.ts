// The tracks, audio tracks and returns of the song: add, name, colour, move and delete them;
// their faders, sends and solo; arming and recording; rendering a track onto an audio track.
import type { StateOfTool } from "../sdk";
import { bus } from "../bus";
import { chain } from "../chain";
import { lfo } from "../lfo";
import { tape } from "../tape";
import { freeName, track } from "../track";
import { choose, copyOf, lengthAt } from "./clips";
import { isRow, isStrip, itemAt, PALETTE, rowOfKey, rowsOf, titleOf, type Child, type Draft, type Song } from "./model";

/** A fader for each return of the song, silent, as a new track sends into them. */
const sendsInto = (draft: Draft) =>
  draft.children.filter((child) => child.tool === "bus").map((back) => ({ name: `send-${back.name}` as const, tool: "fader" as const, state: { gain_db: "-inf" as const } }));

/** Adds an instrument track called `title` after the others, with `clips` on it, in the next colour: its name. */
export function addInstrumentTrack(draft: Draft, title: string, clips: { name: string; state: StateOfTool<"timeline"> }[] = []): string {
  const rows = rowsOf(draft.children, draft.state.order).map((each) => each.name);
  const name = freeName(draft.children.map((child) => child.name), "track");
  draft.children.push({
    name, tool: track, state: { colour: PALETTE[rows.length % PALETTE.length]!.colour, title },
    children: [
      { name: "synth", tool: "instrument.synth", state: {} },
      { name: "chain", tool: chain, state: {} }, { name: "mix", tool: "fader", state: {} }, { name: "rec", tool: "recorder", state: {} },
      ...sendsInto(draft), ...clips.map((clip) => ({ ...clip, tool: "timeline" as const })),
    ],
  });
  draft.state.order = [...rows, name];
  return name;
}

/** What a track is: one of notes, one of sound, or a return the sends of the others play into. */
export type TrackKind = "instrument" | "audio" | "return";

/** The track `name` names, of the record's children. */
const stripIn = (draft: Draft, name: string) => {
  const owner = draft.children.find((child) => child.name === name);
  return owner?.tool === "track" || owner?.tool === "tape" || owner?.tool === "bus" ? owner : undefined;
};
/** The fader `send-<return>` or `mix` of `name`, or the master. */
const faderIn = (draft: Draft, name: string, fader: string) => {
  const owner = draft.children.find((child) => child.name === name);
  const mix = name === "master" ? owner : owner?.tool === "track" || owner?.tool === "tape" || owner?.tool === "bus" ? owner.children.find((child) => child.name === fader) : undefined;
  return mix?.tool === "fader" ? mix : undefined;
};

/**
 * The track the keyboard plays and records on: the chosen one, or the first in `order` while
 * none is; none while an audio track is chosen.
 */
export function notesTrackOf(children: readonly { name: string; tool: unknown }[], order: readonly string[], chosen: string | null): string | null {
  const tracks = rowsOf(children, order).filter((each) => each.tool === "track");
  if (chosen !== null && children.some((child) => child.name === chosen)) return tracks.some((each) => each.name === chosen) ? chosen : null;
  return tracks[0]?.name ?? null;
}

/**
 * Why `each` cannot render onto an audio track, or `undefined` when it can: an envelope that
 * moves its volume or a send hears its notes, which the audio track would not have.
 */
export function cannotRender(each: Child | undefined): string | undefined {
  if (each?.tool !== "track") return undefined;
  const enveloped = each.state.automation.some(({ lane, target }) =>
    (target.startsWith("mix.") || target.startsWith("send-")) && each.children.some((child) => child.name === lane && child.tool === "envelope"));
  return enveloped ? "An envelope moves its volume or a send" : undefined;
}

/** What a view and the keys do to the tracks of the song: each change one undo step. */
export const trackEdits = (song: Song) => {
  const { memory, update } = song;
  const rows = () => rowsOf(song.children, song.state.order);
  const strip = (name: string) => song.children.find((child) => child.name === name);
  const recordable = () => song.children.flatMap((each) => (isRow(each) && each.children.some((child) => child.name === "rec") ? [each.name] : []));
  const record = (name: string, on: boolean) => song.send({ child: `${name}/rec` as const, number: "record" }, on ? 1 : 0);
  /** The track whose panel shows while no clip is chosen: the one "the track" means. */
  const shown = () => (memory.selected.length === 0 && memory.below?.kind === "track" ? memory.below.name : null);
  const edits = {
    /** Adds a track of `kind` after the others; a new return gets a silent send from every track, and the returns open. */
    add(kind: TrackKind) {
      const names = song.children.map((child) => child.name);
      const colour = PALETTE[rows().length % PALETTE.length]!.colour;
      const returns = song.children.filter((child) => child.tool === "bus").length;
      update(kind === "instrument" ? "Add a track" : kind === "audio" ? "Add an audio track" : "Add a return", (draft) => {
        if (kind === "instrument") {
          addInstrumentTrack(draft, `Track ${rows().filter((each) => each.tool === "track").length + 1}`);
        } else if (kind === "audio") {
          const name = freeName(names, "audio-1") as `audio-${string}`;
          const audio = rows().filter((each) => each.tool === "tape").length + 1;
          draft.children.push({ name, tool: tape, state: { colour, title: `Audio ${audio}` }, children: [{ name: "rec", tool: "recorder", state: { channels: [0] } }, { name: "chain", tool: chain, state: {} }, { name: "mix", tool: "fader", state: {} }, ...sendsInto(draft)] });
          draft.state.order = [...rows().map((each) => each.name), name];
        } else {
          const name = [..."abcdefghijklmnopqrstuvwxyz"].map((letter) => `ret-${letter}` as const).find((each) => !names.includes(each)) ?? (freeName(names, "ret-z") as `ret-${string}`);
          draft.children.push({ name, tool: bus, state: { title: `Return ${returns + 1}`, colour: PALETTE[(returns + 3) % PALETTE.length]!.colour }, children: [{ name: "chain", tool: chain, state: {} }, { name: "mix", tool: "fader", state: {} }] });
          const send = { name: `send-${name}` as const, tool: "fader" as const, state: { gain_db: "-inf" as const } };
          for (const each of draft.children) {
            if (each.tool === "track") each.children.push(send);
            else if (each.tool === "tape") each.children.push(send);
          }
        }
      });
      if (kind === "return") memory.returnsOpen = true;
    },
    rename(name: string, title: string | null) {
      memory.renaming = null;
      if (title === null) return;
      update("Rename a track", (draft) => {
        const owner = stripIn(draft, name);
        if (owner) owner.state.title = title.trim();
      });
    },
    colour(name: string, colour: string) {
      update("Colour a track", (draft) => {
        const owner = stripIn(draft, name);
        if (owner) owner.state.colour = colour;
      });
    },
    /** A head dragged onto another, `over` its key at the end: it goes before it. */
    moveBefore(moved: string, before: string) {
      if (moved === before) return;
      update("Move a track", (draft) => {
        const names = rows().map((each) => each.name).filter((name) => name !== moved);
        names.splice(Math.max(names.indexOf(before), 0), 0, moved);
        draft.state.order = names;
      });
    },
    /** Up and down choose the track above or below the one whose panel shows; with `move`, alt-up and alt-down put it there. */
    step(by: -1 | 1, move: boolean): boolean {
      const name = shown();
      const names = rows().map((each) => each.name);
      const at = name === null ? -1 : names.indexOf(name);
      if (name === null || at < 0) return false;
      const next = Math.min(Math.max(at + by, 0), names.length - 1);
      if (!move) {
        memory.below = { kind: "track", name: names[next]! };
        memory.track = names[next]!;
        song.select(names[next]!);
      } else if (next !== at) {
        names.splice(at, 1);
        names.splice(next, 0, name);
        update("Move a track", (draft) => (draft.state.order = names));
      }
      return true;
    },
    /** Shows the panel of the track or return `name` under the timeline, and gives it the keyboard. */
    openPanel(name: string) {
      choose(song, [], name);
      if (memory.below?.kind !== "track" || memory.below.name !== name) memory.below = { kind: "track", name };
    },
    openMaster() {
      memory.below = { kind: "master" };
    },
    /** The mixer shows under the timeline, or goes. */
    toggleMixer() {
      memory.below = memory.below?.kind === "mixer" ? null : { kind: "mixer" };
    },
    /** Deletes the track, audio track or return `name`, with the sends into it, its solo, its sidechains and what chose it. */
    remove(name: string) {
      update("Delete a track", (draft) => {
        draft.state.order = draft.state.order.filter((each) => each !== name);
        draft.state.solo = draft.state.solo.filter((each) => each !== name);
        draft.state.sidechains = draft.state.sidechains.filter(({ effect, from }) => from !== name && !effect.startsWith(`${name}/`));
        for (const each of draft.children) {
          if (each.tool !== "track" && each.tool !== "tape") continue;
          const at = each.children.findIndex((child) => child.name === `send-${name}`);
          if (at >= 0) each.children.splice(at, 1);
        }
        const at = draft.children.findIndex((child) => child.name === name);
        if (at >= 0) draft.children.splice(at, 1);
      });
      if (memory.track === name) memory.track = null;
      const below = memory.below;
      if ((below?.kind === "track" && below.name === name) || (below?.kind === "editor" && rowOfKey(below.clip) === name)) memory.below = null;
      memory.selected = memory.selected.filter((key) => rowOfKey(key) !== name);
      memory.armed = memory.armed.filter((each) => each !== name);
    },
    /** Delete on a track asks first: with no clip chosen the keys go to the track, so one delete too many after a clip's would take it. */
    async askToRemove(name: string) {
      const row = strip(name);
      if (!row || !isStrip(row)) return;
      const title = titleOf(row);
      const said = await song.confirm(
        row.tool === "bus"
          ? { message: `Delete the return "${title}"?`, detail: "Its effects, automation and the sends into it go with it. Undo brings it back.", confirm: "Delete" }
          : { message: `Delete the track "${title}"?`, detail: "Its clips, devices and automation go with it. Undo brings it back.", confirm: "Delete" },
      );
      if (said) edits.remove(name);
    },
    /** The track whose panel shows, which enter renames and delete deletes. */
    shown,
    mute(name: string) {
      update("Mute a track", (draft) => {
        const mix = faderIn(draft, name, "mix");
        if (mix) mix.state.mute = !mix.state.mute;
      });
    },
    solo(name: string) {
      update("Solo a track", (draft) => (draft.state.solo = draft.state.solo.includes(name) ? draft.state.solo.filter((each) => each !== name) : [...draft.state.solo, name]));
    },
    /** The volume of a track, a return or the master, `"-inf"` at the bottom. */
    volume(name: string, db: number | "-inf") {
      update("Change the volume", (draft) => {
        const mix = faderIn(draft, name, "mix");
        if (mix) mix.state.gain_db = db;
      });
    },
    pan(name: string, pan: number) {
      update("Change pan", (draft) => {
        const mix = faderIn(draft, name, "mix");
        if (mix) mix.state.pan = pan;
      });
    },
    /** How loud `name` sends into the return `back`, as "Change <return>". */
    send(name: string, back: string, db: number | "-inf") {
      const target = strip(back);
      update(`Change ${(target ? titleOf(target) : back).toLowerCase()}`, (draft) => {
        const fader = faderIn(draft, name, `send-${back}`);
        if (fader) fader.state.gain_db = db;
      });
    },
    /** Whether the sends into the return `name` take the sound before the tracks' faders. */
    preFader(name: string) {
      const back = strip(name);
      const on = back?.tool === "bus" && back.state.pre_fader;
      update(on ? "Send after the faders" : "Send before the faders", (draft) => {
        const owner = draft.children.find((child) => child.name === name);
        if (owner?.tool === "bus") owner.state.pre_fader = !owner.state.pre_fader;
      });
    },
    limiter() {
      update(song.state.limit_off ? "Turn the limiter on" : "Turn the limiter off", (draft) => (draft.state.limit_off = !draft.state.limit_off));
    },
    /** The channels of the audio input an audio track records. */
    input(name: string, channels: readonly number[]) {
      update("Change input", (draft) => {
        const owner = draft.children.find((child) => child.name === name);
        const rec = owner?.tool === "tape" ? owner.children.find((child) => child.name === "rec") : undefined;
        if (rec?.tool === "recorder") rec.state.channels = [...channels];
      });
    },
    /** Opens or closes the input of the recorder of `name`: while armed its level shows, and a recording takes it, also one that runs. */
    arm(name: string) {
      const on = !memory.armed.includes(name);
      memory.armed = on ? [...memory.armed, name] : memory.armed.filter((each) => each !== name);
      song.send({ child: `${name}/rec` as const, number: "arm" }, on ? 1 : 0);
      if (memory.recording !== null) record(name, on);
    },
    /**
     * Records from the playhead, or ends the recording: every armed track, and the keyboard's
     * notes on its track. With nothing armed and an audio track chosen, it arms that one first.
     */
    record() {
      const all = recordable();
      if (memory.recording !== null) {
        for (const name of all) record(name, false);
        memory.recording = null;
        return;
      }
      const chosen = strip(memory.track ?? "");
      if (memory.armed.length === 0 && chosen?.tool === "tape" && all.includes(chosen.name)) edits.arm(chosen.name);
      const notes = notesTrackOf(song.children, song.state.order, memory.track);
      memory.recording = song.clock.tick;
      for (const name of new Set([...memory.armed, ...(notes ? [notes] : [])])) if (all.includes(name)) record(name, true);
      song.clock.play();
    },
    /**
     * Renders the clips and regions `keys` of the track `name`, from the first to the last, after
     * its effects and before its fader, with the tail or without, onto a new audio track under it
     * that takes its fader, sends, their automation, its solo and what it keys after its fader,
     * and mutes `name`: the song sounds as before.
     */
    async render(name: string, keys: readonly string[], tail: boolean) {
      const source = strip(name);
      const starts = keys.flatMap((key) => itemAt(song.children, key)?.state.start ?? []);
      if (!source || !isRow(source) || starts.length === 0 || cannotRender(source)) return;
      const [from, to] = [Math.min(...starts), Math.max(...keys.map((key) => (itemAt(song.children, key)?.state.start ?? 0) + lengthAt(song, key)))];
      const title = titleOf(source);
      const asset = await song.render(`${name}/chain`, { from, to, tail, name: title });
      if (asset === null) return;
      update("Render to audio track", (draft) => {
        const owner = draft.children.find((child) => child.name === name);
        if (owner?.tool !== "track" && owner?.tool !== "tape") return;
        const audio = freeName(draft.children.map((child) => child.name), "audio-1") as `audio-${string}`;
        // The lanes and LFOs of its faders go along; an envelope hears notes, which an audio track has none of.
        const automation = owner.state.automation.filter(({ target, lane }) =>
          (target.startsWith("mix.") || target.startsWith("send-")) && owner.children.some((child) => child.name === lane && (child.tool === "lane" || child.tool === "lfo")));
        const moved = (child: string) => automation.some(({ lane }) => lane === child);
        const faders = owner.children.flatMap((child) => ((child.name === "mix" || child.name.startsWith("send-")) && child.tool === "fader" ? [{ name: child.name as "mix" | `send-${string}`, tool: "fader" as const, state: { ...child.state } }] : []));
        const lanes = owner.children.flatMap((child) => (moved(child.name) && child.tool === "lane" ? [{ name: child.name as `lane-${string}`, tool: "lane" as const, state: copyOf(child.state) }] : []));
        const lfos = owner.children.flatMap((child) => (moved(child.name) && child.tool === "lfo" ? [{ name: child.name as `lfo-${string}`, tool: lfo, state: copyOf(child.state) }] : []));
        draft.children.push({
          name: audio, tool: tape, state: { colour: owner.state.colour, title: `${title} (audio)`, automation: copyOf(automation) },
          children: [{ name: "rec", tool: "recorder", state: { channels: [0] } }, { name: "chain", tool: chain, state: {} }, ...faders, ...lanes, ...lfos, { name: "render", tool: "region", state: { asset, start: from } }],
        });
        const order = rows().map((each) => each.name);
        order.splice(order.indexOf(name) + 1, 0, audio);
        draft.state.order = order;
        if (draft.state.solo.includes(name)) draft.state.solo = [...draft.state.solo, audio];
        draft.state.sidechains = draft.state.sidechains.map((each) => (each.from === name && each.tap === "post_fader" ? { ...each, from: audio } : each));
        // A lane on its mute would play it again.
        owner.state.automation = owner.state.automation.filter(({ target }) => target !== "mix.mute");
        const mix = owner.children.find((child) => child.name === "mix");
        if (mix?.tool === "fader") mix.state.mute = true;
      });
    },
  };
  return edits;
};
