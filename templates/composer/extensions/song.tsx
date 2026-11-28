import { h, tool } from "./sdk";
import { behaviour } from "./song/behaviour";
import { fields, memory, parts } from "./song/model";
import { trackEdits } from "./song/tracks";
import { pieceEnd } from "./score/bars";
import { scoreEdits } from "./score/edits";
import { actions } from "./score/keys";
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
  doc: "The piece as a score in one line: a staff for each `track` child, with the notes of its clips and no clip around them, under a bars track of the sections, tempo marks, rit. and accel., fermatas and time signatures of `project.json`; audio shows as its sound. The piece ends on the bar line after its last clip. `memory.bars` holds the chosen bars; what they do is in `score/edits.ts`, and bars added or taken out move everything after them (`score/bars.ts`). Each `track` and audio track `audio-*` plays into the fader `master`, then its limiter `limit` unless `limit_off`; `order`, `solo`, the returns `ret-*`, the sends and `sidechains` are in `song/model.ts`. Its keys are its `actions`, in `score/keys.ts` and `song/keys.ts`.",
  state: fields,
  children: parts,
  behaviour,
  memory,
  actions,
  tick: (song) => trackEdits(song).stopped(),
  view: (self, ctx) => {
    const page = openPage(self, ctx);
    const score = { end: pieceEnd(ctx.children, ctx.clock.tempo_map), edits: scoreEdits(ctx) };
    return <SongPage page={page} band={<Marks page={page} score={score} />} time={<Staves page={page} score={score} />} />;
  },
});
