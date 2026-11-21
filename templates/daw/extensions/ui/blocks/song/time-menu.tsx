import { h } from "../../../sdk";
import type { Mark } from "../../../song/model";
import type { Page } from "../../../song/view";
import { MenuList, type Item } from "../../menu";

// What the ruler and the tempo lane share: their menu.

/** A tempo as the ruler and the tempo lane say it, to a tenth. */
export const bpmText = (bpm: number) => String(Math.round(bpm * 10) / 10);

/** What a press on the ruler or the tempo lane keeps for its menu: a right press opens it there, on `mark` when it was on one. */
export const pointed = (page: Page, x: number, mark: Mark | null = null) => (page.song.memory.rulerMenu = { x, mark });

/** Why the menu cannot add a mark where it opened. */
const HERE = "There is one here already";

const NAMES = { section: "section", signature: "time signature change", tempo: "tempo change" } as const;

/** The menu of the ruler and the tempo lane, where it opened: what it adds there, and the mark there it removes. */
export const TimeMenu = ({ page, id, label }: { page: Page; id: string; label: string }) => {
  const { song, edits } = page;
  const { x, mark } = song.memory.rulerMenu;
  const at = edits.marks.menuAt(x, mark ?? undefined);
  const items: Item[] = [
    { text: "Add a tempo change", icon: "tempo-change", ...(at.tempo === null ? { disabled: HERE } : {}), onPress: () => at.tempo !== null && edits.marks.addTempo(at.tempo) },
    { text: "Add a time signature change", icon: "time-signature", ...(at.signature === null ? { disabled: HERE } : {}), onPress: () => at.signature !== null && edits.marks.addSignature(at.signature) },
    { text: "Add a section", icon: "section", ...(at.section === null ? { disabled: HERE } : {}), onPress: () => at.section !== null && edits.marks.addSection(at.section) },
  ];
  const removed = at.removed;
  if (removed) items.push({ text: `Remove the ${NAMES[removed.kind]}`, icon: "trash-2", danger: true, line: true, keys: song.shortcut("delete_mark"), onPress: () => edits.marks.remove(removed) });
  return <MenuList id={id} label={label} items={items} open="menu" w={240} />;
};
