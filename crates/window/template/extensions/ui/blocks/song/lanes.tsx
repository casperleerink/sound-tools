import { h } from "../../../sdk";
import { words } from "../../../song/model";
import { HEAD, type Page, type Row } from "../../../song/view";

/** The rows of one track's automation: a row for each lane, then the one that adds a lane. */
export type LanesProps = { page: Page; rows: readonly Extract<Row, { kind: "lane" | "add-lane" }>[] };

/** The heads of a track's lanes, placed in the heads' plane. */
export const LaneHeads = ({ rows }: LanesProps) => (
  <box>
    {rows.map((row) => (
      <row place={[0, row.top, HEAD - 1, row.height]} px={3} align="center" border="border" borderSides={[0, 0, 1, 0]}>
        <text style="label" color="text-secondary" truncate>{row.kind === "lane" ? words(row.target) : "Add lane"}</text>
      </row>
    ))}
  </box>
);

/** The lines of a track's lanes in time, placed in the timeline's plane. */
export const LaneTimes = ({ page, rows }: LanesProps) => (
  <box>
    {rows.map((row) => <box place={[0, row.top, page.end, row.height]} fill="well" border="border" borderSides={[0, 0, 1, 0]} />)}
  </box>
);
