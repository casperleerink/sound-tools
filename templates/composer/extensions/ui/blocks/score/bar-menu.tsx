import { h } from "../../../sdk";
import { changeBars } from "../../../song/marks";
import { barAt, tickOfBar } from "../../../song/time";
import type { Page } from "../../../song/view";
import { scoreEdits } from "../../../score/edits";
import { MenuList, type Item } from "../../menu";

/** What the score's blocks share: where the piece ends and its edits. */
export type Score = { end: number; edits: ReturnType<typeof scoreEdits> };

/** The bar `tick` is in, from 1, where it starts, and the ticks of a beat of its time signature. */
export function barTicks(page: Page, tick: number) {
  const runs = page.song.clock.tempo_map.time_signatures;
  const { bar, start } = barAt(runs, tick);
  const unit = Number(runs[changeBars(runs).findLastIndex((first) => first <= bar)]?.signature.split("/")[1]) || 4;
  return { bar, start, beat: 3840 / unit };
}

/** Why the menu cannot do something there. */
const [HERE, ONLY] = ["There is one here already", "The piece needs a bar"];

/**
 * The menu of the bar where a right press opened it, on the marks or on the music: bars added
 * before or after it and it taken out, with all after moving; and what starts there: a section,
 * a time signature, a tempo, a rit. or an accel.; and a fermata at its end.
 */
export const BarMenu = ({ page, score, id }: { page: Page; score: Score; id: string }) => {
  const { song, edits } = page;
  const { clock, memory } = song;
  const runs = clock.tempo_map.time_signatures;
  const { bar, start, beat } = barTicks(page, Math.min(Math.max(memory.rulerMenu.x, 0), score.end - 1));
  const fermata = (clock.tempo_map.fermatas ?? []).some((each) => each.tick === tickOfBar(runs, bar + 1) - beat);
  const only = tickOfBar(runs, 2) >= score.end;
  const items: Item[] = [
    { text: "Add a bar before", icon: "plus", onPress: () => score.edits.addBars(bar) },
    { text: "Add a bar after", icon: "plus", onPress: () => score.edits.addBars(bar + 1) },
    { text: "Remove the bar", icon: "trash-2", danger: true, ...(only ? { disabled: ONLY } : {}), onPress: () => score.edits.removeBar(bar) },
    { text: "Start a section here", icon: "section", line: true, ...(clock.sections.some((each) => each.bar === bar) ? { disabled: HERE } : {}), onPress: () => edits.marks.addSection(start) },
    { text: "Change the time signature here", icon: "time-signature", onPress: () => (memory.typing = { kind: "signature", bar }) },
    { text: "Change the tempo here", icon: "tempo-change", onPress: () => score.edits.addTempo(bar) },
    { text: "rit. or accel. from here", icon: "tempo-change", onPress: () => score.edits.addGlide(bar) },
    { text: "Fermata at the end of the bar", icon: "fermata", checked: fermata, onPress: () => score.edits.toggleFermata(bar) },
  ];
  return <MenuList id={id} label={`Bar ${bar}`} items={[{ heading: `Bar ${bar}`, items }]} open="menu" w={260} />;
};
