import { h } from "../../../sdk";
import { itemAt } from "../../../song/model";
import type { Page } from "../../../song/view";
import { Panel } from "../panel";

/** The audio clip `region` names, `row/name`, chosen alone: its gain and fades. */
export type RegionCardProps = { page: Page; region: string };

/** The card of an audio clip in its track's panel. */
export const RegionCard = ({ page, region }: RegionCardProps) => {
  const item = itemAt(page.self.children, region);
  return item?.tool === "region" ? <Panel title="Audio clip"><text style="label" color="text-secondary">{item.state.asset}</text></Panel> : null;
};
