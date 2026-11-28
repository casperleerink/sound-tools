import { h } from "../../../sdk";
import { tickOfBar } from "../../../song/time";
import type { Page } from "../../../song/view";
import { beatOf } from "../../../score/bars";
import { chosenBars, scoreEdits } from "../../../score/edits";
import { MenuList, type Item } from "../../menu";

/** What the score's blocks share: where the piece ends and its edits. */
export type Score = { end: number; edits: ReturnType<typeof scoreEdits> };

/** Why the menu cannot do something there. */
const [HERE, ONLY] = ["There is one here already", "The piece needs a bar"];

/** What a bar or the bars from `first` to `last` are called. */
export const barsOf = (first: number, last: number) => (first === last ? `Bar ${first}` : `Bars ${first} to ${last}`);

/** The menu of the chosen bars. */
export const BarMenu = ({ page, score, id, open = "menu" }: { page: Page; score: Score; id: string; open?: "press" | "menu" }) => {
  const { song, edits } = page;
  const { clock, memory } = song;
  // The right press that opens it chooses its bar first, so the menu is there before a bar is.
  const { first, last } = chosenBars(song) ?? { first: 1, last: 1 };
  const runs = clock.tempo_map.time_signatures;
  const start = tickOfBar(runs, first);
  const fermata = (clock.tempo_map.fermatas ?? []).some((each) => each.tick === tickOfBar(runs, last + 1) - beatOf(runs, last));
  const all = first === 1 && tickOfBar(runs, last + 1) >= score.end;
  const one = first === last;
  const items: Item[] = [
    { text: "Add a bar before", icon: "plus", onPress: () => score.edits.addBars(first) },
    { text: "Add a bar after", icon: "plus", onPress: () => score.edits.addBars(last, 1, true) },
    { text: one ? "Remove the bar" : "Remove the bars", icon: "trash-2", danger: true, keys: song.shortcut("remove_bars"), ...(all ? { disabled: ONLY } : {}), onPress: () => score.edits.removeBars(first, last) },
    { text: "Clear the notes", icon: "eraser", keys: song.shortcut("clear_bars"), onPress: () => score.edits.clearNotes(first, last) },
    { text: "Start a section here", icon: "section", line: true, ...(clock.sections.some((each) => each.bar === first) ? { disabled: HERE } : {}), onPress: () => edits.marks.addSection(start) },
    { text: "Change the time signature", icon: "time-signature", onPress: () => (memory.typing = { kind: "signature", bar: first, last }) },
    { text: "Change the tempo here", icon: "tempo-change", onPress: () => score.edits.addTempo(first) },
    { text: one ? "rit. or accel. over the bar" : "rit. or accel. over the bars", icon: "tempo-change", onPress: () => score.edits.addGlide(first, last) },
    { text: one ? "Fermata at the end of the bar" : "Fermata at the end of the bars", icon: "fermata", checked: fermata, onPress: () => score.edits.toggleFermata(last) },
  ];
  return <MenuList id={id} label={barsOf(first, last)} items={[{ heading: barsOf(first, last), items }]} open={open} w={280} />;
};
