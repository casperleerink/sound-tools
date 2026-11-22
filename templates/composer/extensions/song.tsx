import { h, tool } from "./sdk";
import { behaviour } from "./song/behaviour";
import { actions } from "./song/keys";
import { fields, isRow, memory, parts } from "./song/model";
import { trackEdits } from "./song/tracks";
import { pieceEnd } from "./score/bars";
import { scoreEdits } from "./score/edits";
import { Marks } from "./ui/blocks/score/marks";
import { Staves } from "./ui/blocks/score/staves";
import { openPage, SongPage } from "./ui/blocks/song/page";

// A song read as a score: the whole piece in one line, its form shaped on the music. It plays,
// records and mixes as the tracks page of the DAW does, whose `song/` and `ui/blocks/song/` it
// shares; what it adds is in `score/` and `ui/blocks/score/`.
export const song = tool({
  name: "song",
  title: "Song",
  when: "You change the score page: its instruments, bars, sections, time signatures, tempo marks, rit. and accel., fermatas, panel or mixer",
  doc: "The piece as a score in one line: a staff for each `track` child, with the notes of its clips and no clip around them, under a band of the sections, tempo marks, rit. and accel. and fermatas of `project.json`. It opens on the whole piece, which ends on the bar line after its last clip. A press on a staff opens that instrument's notes there, in a clip from that bar to the end when it has none. A right press on a bar adds bars before or after it or takes it out, everything after moving with them (`score/bars.ts`), or starts a section, time signature, tempo, rit. or accel. or fermata there (`score/edits.ts`). Each `track` and audio track `audio-*` plays into the fader `master`, then its limiter `limit` unless `limit_off`; `order`, `solo`, the returns `ret-*`, the sends and `sidechains` work as on the DAW's song page. Its keys are its `actions`, in `song/keys.ts`.",
  state: fields,
  children: parts,
  behaviour,
  memory,
  actions,
  tick: (song) => trackEdits(song).stopped(),
  view: (self, ctx) => {
    const page = openPage(self, ctx);
    const score = { end: pieceEnd(page.tracks.filter(isRow), ctx.clock.tempo_map.time_signatures), edits: scoreEdits(ctx) };
    return <SongPage page={page} band={<Marks page={page} score={score} />} time={<Staves page={page} score={score} />} />;
  },
});
