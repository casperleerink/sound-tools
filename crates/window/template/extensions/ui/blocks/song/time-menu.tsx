import { h, live, type Num } from "../../../sdk";
import type { Mark } from "../../../song/model";
import { ACROSS, PLANE, type Page } from "../../../song/view";
import { keysOf } from "../../kbd";
import { ContextMenu, type Item } from "../../menu";

// What the ruler and the tempo lane share: their menu, and where a thing of a set width in
// points sits in time.

/** A tempo as the ruler and the tempo lane say it, to a tenth. */
export const bpmText = (bpm: number) => String(Math.round(bpm * 10) / 10);

/** The middle of something `points` wide whose left edge is at `tick`: where a mark of it is placed. */
export const after = (tick: Num, points: number) => live.plane(PLANE).zoom.times(ACROSS).pow(-1).times(points / 2).plus(tick);

/** What a press on the ruler or the tempo lane keeps for its menu: a right press opens it there, on `mark` when it was on one. */
export const pointed = (page: Page, x: number, mark: Mark | null = null) => (page.song.memory.rulerMenu = { x, mark });

const NAMES = { section: "section", signature: "time signature change", tempo: "tempo change" } as const;

/** The menu of the ruler and the tempo lane, where it opened: what it adds there, and the mark there it removes. */
export const TimeMenu = ({ page, id, label }: { page: Page; id: string; label: string }) => {
  const { song, edits } = page;
  const { x, mark } = song.memory.rulerMenu;
  const at = edits.marks.menuAt(x, mark ?? undefined);
  const items: Item[] = [
    { text: "Add a tempo change", icon: "tempo-change", disabled: at.tempo === null, onPress: () => at.tempo !== null && edits.marks.addTempo(at.tempo) },
    { text: "Add a time signature change", icon: "time-signature", disabled: at.signature === null, onPress: () => at.signature !== null && edits.marks.addSignature(at.signature) },
    { text: "Add a section", icon: "section", disabled: at.section === null, onPress: () => at.section !== null && edits.marks.addSection(at.section) },
  ];
  const removed = at.removed;
  if (removed) items.push({ text: `Remove the ${NAMES[removed.kind]}`, icon: "trash-2", danger: true, line: true, keys: keysOf(song.shortcut("delete_mark")), onPress: () => edits.marks.remove(removed) });
  return <ContextMenu id={id} label={label} w={240} items={items} />;
};
