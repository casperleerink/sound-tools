// Edits of notes, as a piano roll makes them: each gives new notes in the order of the notes it
// was given, so a caller keeps which are chosen by their place. Ticks count from where the notes
// are played from, as in a `timeline`; keeping them inside a clip is the caller's.

import { Note } from "./fields";

/**
 * Moves each start `strength` of the way, 0 to 1, to the nearest line of a grid `grid` ticks
 * apart, from tick 0. `swing`, 0 to 1, moves every second line later by up to a third of the
 * grid, which at 1 is a triplet feel; the nearest line is the nearest swung one. Lengths stay.
 */
export function quantize(notes: readonly Note[], { grid, strength = 1, swing = 0 }: { grid: number; strength?: number; swing?: number }): Note[] {
  if (!(grid > 0)) throw new Error(`quantize needs a grid of 1 tick or more, not ${grid}`);
  const line = (index: number) => index * grid + (index % 2 === 1 ? (swing * grid) / 3 : 0);
  return notes.map((note) => {
    const near = Math.floor(note.start / grid);
    const lines = [near - 1, near, near + 1].filter((index) => index >= 0).map(line);
    const target = lines.reduce((best, each) => (Math.abs(each - note.start) < Math.abs(best - note.start) ? each : best));
    return { ...note, start: Math.max(0, Math.round(note.start + (target - note.start) * strength)) };
  });
}

/**
 * Moves each start by up to `timing` ticks either way and each velocity by up to `velocity`
 * either way, at random from `seed`: the same seed and notes give the same notes. A start stays
 * at 0 or after, a velocity from 1 to 127.
 */
export function humanize(notes: readonly Note[], { seed, timing, velocity }: { seed: number; timing: number; velocity: number }): Note[] {
  const random = mulberry32(seed);
  const spread = (most: number) => Math.round((random() * 2 - 1) * most);
  return notes.map((note) => ({
    ...note,
    start: Math.max(0, note.start + spread(timing)),
    velocity: Math.min(127, Math.max(1, note.velocity + spread(velocity))),
  }));
}

/**
 * Ends each note where the next later start of `notes` begins, whatever its pitch, so the
 * notes of a chord all reach the next one; the notes that start last keep their length.
 */
export function legato(notes: readonly Note[]): Note[] {
  const starts = [...new Set(notes.map((note) => note.start))].sort((a, b) => a - b);
  return notes.map((note) => {
    const next = starts.find((start) => start > note.start);
    return next === undefined ? { ...note } : { ...note, length: next - note.start };
  });
}

/** A small random number generator from 0 to 1 that a seed repeats. */
function mulberry32(seed: number) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let mixed = state;
    mixed = Math.imul(mixed ^ (mixed >>> 15), mixed | 1);
    mixed ^= mixed + Math.imul(mixed ^ (mixed >>> 7), mixed | 61);
    return ((mixed ^ (mixed >>> 14)) >>> 0) / 4294967296;
  };
}
