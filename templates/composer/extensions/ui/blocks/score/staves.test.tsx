import { expect, test } from "bun:test";
import { find, h, render } from "../../../sdk";
import { pieceEnd } from "../../../score/bars";
import { scoreEdits } from "../../../score/edits";
import { clipOf, fakeSong, trackOf } from "../../../song/fake";
import { BAR } from "../../../song/time";
import { pageOf } from "../../../song/view";
import { Staves } from "./staves";

test("a note is drawn as long as it sounds: to the end of its clip", () => {
  const f = fakeSong({ children: [trackOf("t00", [clipOf("music", 0, BAR, [{ start: 0, length: 2 * BAR, pitch: 60, velocity: 100 }]), clipOf("silence", BAR, BAR)])] });
  const song = Object.assign(Object.create(f.song), { shortcut: () => null });
  const self = { name: "song", state: f.song.state, children: f.song.children, facts: { title: "Song", fields: [] } };
  const score = { end: pieceEnd(song.children, song.clock.tempo_map), edits: scoreEdits(song) };
  render((_self: typeof self) => <Staves page={pageOf(self as never, song)} score={score} />, self);
  const notes = find("notes-t00") as { rects: { rects: number[] }[] } | undefined;
  expect(notes?.rects[0]?.rects[2]).toBe(BAR);
});
