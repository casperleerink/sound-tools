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
    const chosen = chosenBars(song.memory);
    if (!chosen || song.memory.notes.length > 0) return false;
    run(song, chosen);
  },
});

export const actions: Record<string, Action<Song>> = {
  clear_bars: onBars(["backspace", "delete"], "Clear the notes of the chosen bars", (song, { first, last }) => scoreEdits(song).clearNotes(first, last)),
  remove_bars: onBars(["cmd-backspace", "cmd-delete"], "Remove the chosen bars and close the gap", (song, { first, last }) => void scoreEdits(song).removeBars(first, last)),
  let_go_of_bars: onBars(["escape"], "Let go of the chosen bars", ({ memory }) => void (memory.bars = null)),
  ...songActions,
};
