import { h, memo, type Child } from "../../../sdk";
import { colourOf, muted, titleOf } from "../../../song/model";
import { HEAD, LOW_ROW, PLANE, ROW, laneGroups, type Page, type Row, type Strip } from "../../../song/view";
import { Dot } from "../../dot";
import { Meter } from "../../meter";
import { Toggle } from "../../toggle";
import { LaneHeads } from "./lanes";

export type HeadsProps = { page: Page };

/** The column left of the timeline: a head for each track and its lanes, then the returns and the master. */
export const Heads = ({ page }: HeadsProps) => {
  const { song, rows, returns } = page;
  return (
    <col w={HEAD} h="full" border="border" borderSides={[0, 1, 0, 0]}>
      <box key="heads" label="Tracks" grow={1} w="full" plane={{ y: PLANE }}>
        {rows.map((row) => (row.kind === "track" ? <Head page={page} each={row.track} row={row} /> : null))}
        {laneGroups(rows).map((lanes) => <LaneHeads page={page} rows={lanes} />)}
      </box>
      {returns.map((each) => <Low page={page} each={each} />)}
      <row key="master" label="Master" role="tab" checked={song.memory.below?.kind === "master"} w="full" h={LOW_ROW} px={3} gap={2} align="center" border="border" borderSides={[1, 0, 0, 0]} hover="fill" onPress={() => page.edits.tracks.openMaster()}>
        <box w={8} h={8} radius="full" border="text-secondary" />
        <text style="heading" color="text">Master</text>
      </row>
    </col>
  );
};

/** The head of a track or an audio track: a press chooses it and shows its panel. */
const Head = ({ page, each, row }: { page: Page; each: Strip; row: Row }) => {
  const { song, edits } = page;
  const title = titleOf(each);
  const chosen = song.memory.below?.kind === "track" && song.memory.below.name === each.name;
  const [isMuted, soloed, armed, colour] = [muted(each), song.state.solo.includes(each.name), song.memory.armed.includes(each.name), colourOf(each)];
  return memo(`head-${each.name}`, [title, chosen, isMuted, soloed, armed, colour, row.top], () => (
    <row key={`track-${each.name}`} label={`Choose ${title}`} role="tab" checked={chosen} hoverGroup place={[0, row.top, HEAD - 1, 1]} px={3} gap={2} align="center" fill={chosen ? "selected" : undefined} hover={chosen ? undefined : "fill/50"} border="border" borderSides={[0, 0, 1, 0]} onPress={() => edits.tracks.openPanel(each.name)}>
      <Dot color={colour} />
      <col grow={1} gap={1}>
        <text style="heading" color={chosen ? "text" : "text-secondary"} truncate>{title}</text>
        <row gap={1}>
          <Quiet shown={isMuted || chosen}><Toggle id={`mute-${each.name}`} label={`Mute ${title}`} text="M" on={isMuted} color="warning" onToggle={() => edits.tracks.mute(each.name)} /></Quiet>
          <Quiet shown={soloed || chosen}><Toggle id={`solo-${each.name}`} label={`Solo ${title}`} text="S" on={soloed} color="hot" onToggle={() => edits.tracks.solo(each.name)} /></Quiet>
          {each.tool === "tape" && <Quiet shown={armed || chosen}><Toggle id={`arm-${each.name}`} label={`Arm ${title}`} on={armed} color="danger" dot onToggle={() => edits.tracks.arm(each.name)} /></Quiet>}
        </row>
      </col>
      <Meter id={`meter-head-${each.name}`} label={`Level ${title}`} path={`${each.name}/mix`} w={4} h={ROW - 16} />
    </row>
  ));
};

/** The row of a return under the heads: a press shows its panel. */
const Low = ({ page, each }: { page: Page; each: Strip }) => {
  const title = titleOf(each);
  const isMuted = muted(each);
  return (
    <row key={`track-${each.name}`} label={`Choose ${title}`} role="tab" checked={page.song.memory.below?.kind === "track" && page.song.memory.below.name === each.name} hoverGroup w="full" h={LOW_ROW} px={3} gap={2} align="center" border="border" borderSides={[1, 0, 0, 0]} hover="fill/50" onPress={() => page.edits.tracks.openPanel(each.name)}>
      <Dot color={colourOf(each)} />
      <text style="heading" color="text-secondary" truncate>{title}</text>
      <row grow={1} justify="end">
        <Quiet shown={isMuted}><Toggle id={`mute-${each.name}`} label={`Mute ${title}`} text="M" on={isMuted} color="warning" onToggle={() => page.edits.tracks.mute(each.name)} /></Quiet>
      </row>
    </row>
  );
};

/** A toggle of a head shows while on, or while its track is chosen or pointed at. */
const Quiet = ({ shown, children }: { shown: boolean; children?: Child }) => (shown ? <box>{children}</box> : <box reveal>{children}</box>);
