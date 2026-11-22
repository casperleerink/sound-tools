import { h, tool } from "./sdk";
import { Ruler } from "./ui/blocks/song/ruler";
import { openPage, SongPage } from "./ui/blocks/song/page";
import { Timeline } from "./ui/blocks/song/timeline";
import { behaviour } from "./song/behaviour";
import { actions } from "./song/keys";
import { fields, memory, parts } from "./song/model";
import { trackEdits } from "./song/tracks";

// A song: tracks of clips in time, and under them the devices of the chosen track, the notes of a
// clip or the mixer. What it does is in `song/`; its blocks, in `ui/blocks/song/`, draw it.
export const song = tool({
  name: "song",
  title: "Song",
  when: "You change the song page: its tracks, clips, panel or mixer",
  doc: "Tracks of clips in time, the devices of the chosen track under them, and a mixer. Each `track` child and each audio track `audio-*` plays into the fader `master`, then its limiter `limit` unless `limit_off`; `order` is the order of the tracks, `solo` names the tracks that play alone. Each return `ret-*` plays what the sends `send-<return>` of the tracks bring into the master too. A track's arm opens its recorder `rec`; R records every armed track and the keyboard's notes on the chosen track. Each of `sidechains` keys an effect, by its path such as `t00/chain/comp`, with what a track `from` plays at `tap`. Its keys are its `actions`, in `song/keys.ts`; what they and the view change, in `song/edits.ts`.",
  state: fields,
  children: parts,
  behaviour,
  memory,
  actions,
  tick: (song) => trackEdits(song).stopped(),
  view: (self, ctx) => {
    const page = openPage(self, ctx);
    return <SongPage page={page} band={<Ruler page={page} />} time={<Timeline page={page} />} />;
  },
});
