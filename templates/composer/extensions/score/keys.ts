// What the score does on a key while bars are chosen, tried before the song's own keys. With
// notes chosen in the editor, the keys are theirs.
import type { Action, ActionKey } from "../sdk";
import { actions as songActions } from "../song/keys";
import type { Song } from "../song/model";
import { chosenBars, scoreEdits } from "./edits";

const onBars = (keys: readonly ActionKey[], title: string, run: (song: Song, chosen: NonNullable<ReturnType<typeof chosenBars>>) => void): Action<Song> => ({
  title,
  keys,
  run: (song) => {
    const chosen = chosenBars(song);
    if (!chosen || song.memory.notes.length > 0) return false;
    run(song, chosen);
  },
});

// The score draws no clips, and its tempo and time signature changes go by bars.
const unseen = new Set(["add_tempo", "add_signature", "repeat", "show_chosen", "next_clip", "previous_clip", "add_clip", "delete_clips", "cut_clips", "copy_clips", "duplicate_clips", "open_clip", "select_all_clips", "paste_clips", "clips_left", "clips_right", "clips_up", "clips_down"]);

export const actions: Record<string, Action<Song>> = {
  clear_bars: onBars(["backspace", "delete"], "Clear the notes of the chosen bars", (song, { first, last }) => scoreEdits(song).clearNotes(first, last)),
  remove_bars: onBars(["cmd-backspace", "cmd-delete"], "Remove the chosen bars and close the gap", (song, { first, last }) => void scoreEdits(song).removeBars(first, last)),
  let_go_of_bars: onBars(["escape"], "Let go of the chosen bars", ({ memory }) => void (memory.bars = null)),
  ...Object.fromEntries(Object.entries(songActions).filter(([name]) => !unseen.has(name))),
};
