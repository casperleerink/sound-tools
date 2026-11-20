// What the parts of the note editor share to line up: their sizes, the planes and drags they
// share, and where the roll of a clip opens. Across, the roll is in ticks of the clip; down, in
// rows from the row it opened on, `top`: a pitch is on row `127 - pitch - top`, and the edits
// take `127 - pitch`.
import { live, type Frozen, type Note } from "../../../sdk";
import { lengthOf } from "../../../song/clips";
import { colourOf, itemAt, rowOfKey } from "../../../song/model";
import { noteKey } from "../../../song/notes";
import { BAR, BEAT, barStarts } from "../../../song/time";
import type { Page } from "../../../song/view";

/** The header, the lane under the notes, the keys and a black key across, and a pitch's row, in points. */
export const [HEADER, LANE, KEYS, BLACK_KEY, PITCH] = [32, 64, 40, 24, 12];
/** Points per tick at the closest zoom and the farthest. */
const [CLOSEST, FARTHEST] = [0.2, 0.0025];
/** How wide the roll is taken to be when it opens, and the room it leaves after the clip. */
const [WIDE, AFTER] = [1100, 96];
/** How far the roll reaches past the end of the clip, and before its start. */
export const [PAST, BEFORE] = [8 * BEAT, BEAT];
/** The shortest a note gets: 1/64. */
export const SHORTEST = BEAT / 16;
/** The drags the chosen notes share: a move, a drag of their starts or ends, and of their velocities. */
export const [MOVE, START, END, VELOCITY] = ["notes-move", "notes-start", "notes-end", "notes-velocity"];

const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);

/** Where the roll of a clip opens in an editor `height` points tall: the middle of its notes in the middle, on a whole row, and fitted across. */
export function openingOf(notes: readonly Frozen<Note>[], length: number, height: number) {
  const pitches = notes.map((note) => note.pitch);
  const middle = pitches.length > 0 ? (Math.min(...pitches) + Math.max(...pitches)) / 2 : 60;
  const rows = Math.max(height - HEADER - LANE, PITCH) / PITCH;
  const top = clamp(Math.round(127 - middle + 0.5 - rows / 2), 0, Math.max(Math.floor(128 - rows), 0));
  return { top, across: clamp((WIDE - AFTER) / Math.max(length, BEAT), FARTHEST, CLOSEST) };
}

/** What every part of the editor of the clip `clip` gets, or `null` while it is no clip. */
export function rollOf(page: Page, clip: string) {
  const item = itemAt(page.self.children, clip);
  const track = page.tracks.find((each) => each.name === rowOfKey(clip));
  if (item?.tool !== "timeline" || !track) return null;
  const { song } = page;
  const state = item.state;
  const length = lengthOf(state);
  const opened = (song.memory.rolls[clip] ??= openingOf(state.notes, length, song.memory.editor));
  const runs = song.clock.tempo_map.time_signatures;
  return {
    page,
    clip,
    state,
    track,
    length,
    end: length + PAST,
    ...opened,
    /** The key of the roll's plane: the keys, the ruler and the lanes follow it. */
    key: `roll-${clip.replace("/", "-")}`,
    colour: colourOf(track),
    chosen: new Set(song.memory.notes),
    edits: page.edits.notes(clip),
    runs,
    /** The lines of the bars of the song from before the clip to past it, in its ticks, and their numbers. */
    bars: barStarts(runs, state.start + length + PAST).flatMap((tick, index) => (tick >= state.start - BEFORE ? [{ tick: tick - state.start, bar: index + 1 }] : [])),
    hear: (pitch: number) => song.play(pitch, { child: track.name as `audio-${string}`, seconds: 0.3 }),
  };
}
export type Roll = NonNullable<ReturnType<typeof rollOf>>;

/** How far the roll zooms, about where it opened. */
export const ZOOM: [number, number] = [FARTHEST, CLOSEST];

/** The notes of the clip with their places in its list, a list each bar: a memo a bar, so an edit draws one bar again. */
export function barsOf(notes: readonly Frozen<Note>[]) {
  const bars = new Map<number, { note: Frozen<Note>; index: number }[]>();
  notes.forEach((note, index) => {
    const bar = Math.floor(note.start / BAR);
    bars.set(bar, [...(bars.get(bar) ?? []), { note, index }]);
  });
  return [...bars];
}

/** What a memo of the notes of a bar compares: the notes, and which are chosen. */
export const signatureOf = (roll: Roll, notes: readonly { note: Frozen<Note>; index: number }[]) =>
  notes.map(({ note, index }) => `${index}=${noteKey(note)}${roll.chosen.has(noteKey(note)) ? "*" : ""}`).join(",");

/** 1 while the playhead is inside the clip, and where it is in the clip's ticks: a looping clip's round. */
export function playheadOf(roll: Roll) {
  const { start } = roll.state;
  const into = live.playhead.minus(start);
  return roll.state.loop
    ? { at: into.max(0).mod(roll.length), inside: live.playhead.ge(start) }
    : { at: into, inside: live.playhead.ge(start).times(live.playhead.lt(start + roll.length)) };
}
