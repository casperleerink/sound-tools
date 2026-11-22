import { expect, test } from "bun:test";
import { clipOf, laneOf, regionOf, tapeOf, trackOf } from "../song/fake";
import { BAR, BEAT } from "../song/time";
import { moveMusic, pieceEnd, withBars, type Item } from "./bars";

const note = (start: number, length = BEAT) => ({ start, length, pitch: 60, velocity: 100 });
/** Tracks as an update gets them. */
const tracksOf = (...tracks: unknown[]) => tracks as Item[];
/** The state of what `track` holds at `index`, the fader first. */
const stateOf = <T>(tracks: Item[], track: number, index: number) => tracks[track]?.children?.[index]?.state as T;

/** Eight bars of 4/4 then 3/4; sections at bars 1 and 9; a rit. in bar 9 to bar 10; a fermata on the last beat of bar 8. */
const piece = () => ({
  tempo_map: {
    time_signatures: [{ signature: "4/4", bars: 8 }, { signature: "3/4", bars: 1 }],
    tempo_changes: [{ tick: 0, bpm: 120 }, { tick: 8 * BAR, bpm: 90, to: 60 }, { tick: 8 * BAR + 3 * BEAT, bpm: 60 }],
    fermatas: [{ tick: 7 * BAR + 3 * BEAT, hold: 2 }],
  },
  sections: [{ bar: 1, name: "Opening" }, { bar: 9, name: "Theme" }],
});

test("a bar added before bar 5 moves everything after it a bar later, and its run of bars grows", () => {
  const added = withBars(piece(), 5, 1);
  expect(added.tempo_map.time_signatures).toEqual([{ signature: "4/4", bars: 9 }, { signature: "3/4", bars: 1 }]);
  expect(added.tempo_map.tempo_changes.map((change) => change.tick)).toEqual([0, 9 * BAR, 9 * BAR + 3 * BEAT]);
  expect(added.tempo_map.tempo_changes[1]).toMatchObject({ bpm: 90, to: 60 });
  expect(added.tempo_map.fermatas).toEqual([{ tick: 8 * BAR + 3 * BEAT, hold: 2 }]);
  expect(added.sections).toEqual([{ bar: 1, name: "Opening" }, { bar: 10, name: "Theme" }]);

  const tracks = tracksOf(trackOf("t00", [clipOf("music", 0, 10 * BAR, [note(BAR), note(4 * BAR), note(9 * BAR)]), laneOf("lane", [{ tick: 3 * BAR, value: 0 }, { tick: 6 * BAR, value: 1 }])]), tapeOf("audio-1", [regionOf("voice", 6 * BAR)]));
  moveMusic(tracks, 4 * BAR, BAR);
  const [clip, lane] = [stateOf<{ length: number; notes: { start: number }[] }>(tracks, 0, 1), stateOf<{ points: { tick: number }[] }>(tracks, 0, 2)];
  expect(clip.notes.map((each) => each.start)).toEqual([BAR, 5 * BAR, 10 * BAR]);
  expect(clip.length).toBe(11 * BAR);
  expect(lane.points.map((each) => each.tick)).toEqual([3 * BAR, 7 * BAR]);
  expect(stateOf<{ start: number }>(tracks, 1, 1).start).toBe(7 * BAR);
});

test("a bar taken out takes its notes, its tempo change and its fermata, cuts a held note, and keeps the tempo it set", () => {
  const taken = withBars(piece(), 8, -1);
  expect(taken.tempo_map.time_signatures).toEqual([{ signature: "4/4", bars: 7 }, { signature: "3/4", bars: 1 }]);
  expect(taken.tempo_map.tempo_changes.map((change) => change.tick)).toEqual([0, 7 * BAR, 7 * BAR + 3 * BEAT]);
  expect(taken.tempo_map.fermatas).toBeUndefined();
  expect(taken.sections).toEqual([{ bar: 1, name: "Opening" }, { bar: 8, name: "Theme" }]);

  // A change inside the bar still plays after it, from where the bar was.
  const inside = withBars({ ...piece(), tempo_map: { ...piece().tempo_map, tempo_changes: [{ tick: 0, bpm: 120 }, { tick: 2 * BAR + BEAT, bpm: 70 }] } }, 3, -1);
  expect(inside.tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 120 }, { tick: 2 * BAR, bpm: 70 }]);

  const tracks = tracksOf(trackOf("t00", [clipOf("music", 0, 4 * BAR, [note(BAR - BEAT, 2 * BEAT), note(BAR + BEAT), note(3 * BAR)])]));
  moveMusic(tracks, BAR, -BAR);
  const clip = stateOf<{ length: number; notes: { start: number; length: number }[] }>(tracks, 0, 1);
  expect(clip.notes).toEqual([{ ...note(BAR - BEAT), length: BEAT }, note(2 * BAR)]);
  expect(clip.length).toBe(3 * BAR);
});

test("the piece ends on the bar line after its music, and bars added at its end are in its clips", () => {
  const runs = [{ signature: "4/4", bars: 1 }];
  expect(pieceEnd(tracksOf(trackOf("t00", [clipOf("music", 0, 6 * BAR - BEAT)])), runs)).toBe(6 * BAR);
  expect(pieceEnd(tracksOf(trackOf("t00")), runs)).toBe(16 * BAR);
  const ending = tracksOf(trackOf("t00", [clipOf("music", 0, 6 * BAR)]));
  moveMusic(ending, 6 * BAR, 2 * BAR);
  expect(pieceEnd(ending, runs)).toBe(8 * BAR);
});
