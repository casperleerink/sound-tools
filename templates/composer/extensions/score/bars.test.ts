import { expect, test } from "bun:test";
import { clipOf, laneOf, regionOf, tapeOf, trackOf } from "../song/fake";
import { BAR, BEAT } from "../song/time";
import type { Child, Draft } from "../song/model";
import { moveMusic, pieceEnd, withBars } from "./bars";

const note = (start: number, length = BEAT) => ({ start, length, pitch: 60, velocity: 100 });
/** Tracks as an update gets them. */
const tracksOf = (...tracks: unknown[]) => tracks as Draft["children"];
/** The state of what `track` holds at `index`, the fader first. */
const stateOf = <T>(tracks: Draft["children"], track: number, index: number) => (tracks as unknown as { children: { state: T }[] }[])[track]?.children[index]?.state as T;

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
  // The only bar of a run between two of one time signature leaves one run.
  const between = { ...piece(), tempo_map: { ...piece().tempo_map, time_signatures: [{ signature: "4/4", bars: 2 }, { signature: "3/4", bars: 1 }, { signature: "4/4", bars: 1 }] } };
  expect(withBars(between, 3, -1).tempo_map.time_signatures).toEqual([{ signature: "4/4", bars: 3 }]);

  // A change inside the bar still plays after it, from where the bar was, unless the bar after it changes the tempo too.
  const changes = (...tempo_changes: { tick: number; bpm: number }[]) => ({ ...piece(), tempo_map: { ...piece().tempo_map, tempo_changes } });
  expect(withBars(changes({ tick: 0, bpm: 120 }, { tick: 2 * BAR + BEAT, bpm: 70 }), 3, -1).tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 120 }, { tick: 2 * BAR, bpm: 70 }]);
  expect(withBars(changes({ tick: 0, bpm: 120 }, { tick: BAR, bpm: 70 }, { tick: 2 * BAR, bpm: 90 }), 2, -1).tempo_map.tempo_changes).toEqual([{ tick: 0, bpm: 120 }, { tick: BAR, bpm: 90 }]);

  const tracks = tracksOf(trackOf("t00", [clipOf("music", 0, 4 * BAR, [note(BAR - BEAT, 2 * BEAT), note(BAR + BEAT), note(3 * BAR)])]));
  moveMusic(tracks, BAR, -BAR);
  const clip = stateOf<{ length: number; notes: { start: number; length: number }[] }>(tracks, 0, 1);
  expect(clip.notes).toEqual([{ ...note(BAR - BEAT), length: BEAT }, note(2 * BAR)]);
  expect(clip.length).toBe(3 * BAR);
});

test("a bar added inside a clip makes no note that its end cut short sound longer", () => {
  const tracks = tracksOf(trackOf("t00", [clipOf("music", 0, 4 * BAR, [note(0, 8 * BAR)])]));
  moveMusic(tracks, BAR, BAR);
  expect(stateOf<{ length: number; notes: { length: number }[] }>(tracks, 0, 1)).toMatchObject({ length: 5 * BAR, notes: [{ length: 4 * BAR }] });
});

test("the piece ends on the bar line after its music, audio as long as it plays, and a clip that ends where bars go in keeps its end", () => {
  const tempo_map = { time_signatures: [{ signature: "4/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 120 }] };
  const end = (tracks: Draft["children"]) => pieceEnd(tracks as unknown as Child[], tempo_map);
  expect(end(tracksOf(trackOf("t00", [clipOf("music", 0, 6 * BAR - BEAT)])))).toBe(6 * BAR);
  expect(end(tracksOf(trackOf("t00")))).toBe(16 * BAR);
  // A minute at 120 is 30 bars.
  expect(end(tracksOf(tapeOf("audio-1", [regionOf("voice", 0, 60)])))).toBe(30 * BAR);
  const ending = tracksOf(trackOf("t00", [clipOf("music", 0, 6 * BAR)]));
  moveMusic(ending, 6 * BAR, 2 * BAR);
  expect(end(ending)).toBe(6 * BAR);
});
