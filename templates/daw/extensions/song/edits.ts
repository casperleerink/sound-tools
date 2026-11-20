// The one surface a view calls with what the pointer said: each call is one undo step, and the
// view needs to know nothing of how the records hold it.
import { clipEdits } from "./clips";
import { effectEdits } from "./effects";
import { laneEdits } from "./lanes";
import { markEdits } from "./marks";
import type { Song } from "./model";
import { noteEdits } from "./notes";
import { regionEdits } from "./regions";
import { trackEdits } from "./tracks";

export const editsOf = (song: Song) => ({
  clips: clipEdits(song),
  regions: regionEdits(song),
  /** The notes of the clip `key`, `row/clip`. */
  notes: (key: string) => noteEdits(song, key),
  lanes: laneEdits(song),
  marks: markEdits(song),
  tracks: trackEdits(song),
  /** The devices of the track or return `name`. */
  effects: (name: string) => effectEdits(song, name),
});
