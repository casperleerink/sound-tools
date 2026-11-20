import { h, live, memo, type Color, type Lines } from "../../../sdk";
import { lengthAt, lengthOf } from "../../../song/clips";
import { colourOf } from "../../../song/model";
import { BEAT } from "../../../song/time";
import { ACROSS, GUTTER, PLANE, ROW, ZOOM, laneGroups, type Page, type Row, type Track } from "../../../song/view";
import { Button } from "../../button";
import { EmptyState } from "../empty-state";
import { LaneTimes } from "./lanes";

export type TimelineProps = { page: Page };

/** The tracks in time: their clips and regions on the plane the ruler shares, the playhead over them, and the foot beside the returns and the master. */
export const Timeline = ({ page }: TimelineProps) => {
  const { rows, tracks, end, height, low } = page;
  const view = live.plane(PLANE);
  return (
    <col grow={1} h="full">
      <row grow={1} w="full">
        <box w={GUTTER} h="full" />
        <box key={PLANE} label="Timeline" grow={1} h="full" plane={{ x: ACROSS, y: ROW, zoom: ZOOM }}>
          <box place={[0, 0, end, height]} />
          {rows.map((row) => (row.kind === "track" ? <TrackTime page={page} track={row.track} row={row} /> : null))}
          {laneGroups(rows).map((lanes) => <LaneTimes page={page} rows={lanes} />)}
        </box>
      </row>
      <box w="full" h={low} border="border" borderSides={[1, 0, 0, 0]} />
      {tracks.length === 0 && <EmptySong page={page} />}
      <box inset={[GUTTER, 0, 0, 0]} clip inert>
        <box key="playhead" inset={[0, 0, null, 0]} w={1} fill="text" at={[live.playhead.minus(view.left).times(view.zoom.times(ACROSS)), 0]} />
      </box>
    </col>
  );
};

/** The clips or the regions of one track, on its row. */
const TrackTime = ({ page, track, row }: { page: Page; track: Track; row: Row }) => {
  const { song, end } = page;
  const colour = colourOf(track);
  const tempo = song.clock.tempo_map;
  return memo(`time-${track.name}`, [track.children, colour, tempo, row.top, end], () => (
    <box place={[0, row.top, end, 1]} border="border" borderSides={[0, 0, 1, 0]}>
      {track.children.flatMap((item) => {
        if (item.tool === "timeline") return [<Clip name={item.state.title || ""} start={item.state.start} length={lengthOf(item.state)} top={row.top} notes={item.state.notes} colour={colour} />];
        if (item.tool === "region") {
          const length = lengthAt({ children: [track], clock: { tempo_map: tempo } }, `${track.name}/${item.name}`);
          return [<Clip name={item.name} start={item.state.start} length={Math.max(length, BEAT)} top={row.top} notes={[]} colour={colour} />];
        }
        return [];
      })}
    </box>
  ));
};

/** A clip: its notes in ticks across and pitches down, so they stretch with it. */
const Clip = ({ name, start, length, top, notes, colour }: { name: string; start: number; length: number; top: number; notes: readonly { start: number; length: number; pitch: number }[]; colour: Color }) => {
  const [low, high] = [Math.min(...notes.map((note) => note.pitch), 60), Math.max(...notes.map((note) => note.pitch), 72)];
  const points = notes.flatMap((note) => [note.start, high - note.pitch, note.start + note.length, high - note.pitch]);
  const lines: Lines[] = [{ color: colour, width: 2, points }];
  return (
    <box place={[start, top + 4 / ROW, length, (ROW - 8) / ROW]} radius="sm" fill={`${colour}/15` as Color} border={`${colour}/60` as Color} clip>
      <text at={[6, 2]} style="label" color="text">{name}</text>
      <shapes place={[start, top + 24 / ROW, length, (ROW - 32) / ROW]} units={[length, high - low + 1]} lines={lines} />
    </box>
  );
};

/** What the timeline says while the song has no track. */
const EmptySong = ({ page }: { page: Page }) => (
  <box inset={[0, 0, 0, 0]}>
    <EmptyState title="An empty song" text="Add a track to play or record, or ask the agent for one.">
      <row gap={2}>
        <Button id="empty-instrument" label="Add an instrument track" text="Instrument track" onPress={() => page.edits.tracks.add("instrument")} />
        <Button id="empty-audio" label="Add an audio track" text="Audio track" onPress={() => page.edits.tracks.add("audio")} />
      </row>
    </EmptyState>
  </box>
);
