import { h, hearsNotes, live, Live, memo, type Color } from "../../../sdk";
import { lengthAt, lengthOf } from "../../../song/clips";
import { colourOf, itemAt, muted, rowOfKey, titleOf, words } from "../../../song/model";
import { cannotRender } from "../../../song/tracks";
import { BAR, barStarts, BEAT, dragGrid, linesIn, placeOf, stepAt, type Runs } from "../../../song/time";
import { GUTTER, laneGroups, PLANE, ROW, TIME, trackAt, trackY, type Page, type Row, type Track } from "../../../song/view";
import { Button } from "../../button";
import { MenuList, type Item } from "../../menu";
import { TextField } from "../../text-field";
import { Tooltip } from "../../tooltip";
import { EmptyState } from "../empty-state";
import { Clip, Grid, INSET, Playhead } from "../time";
import { DIM, renders } from "./heads";
import { LaneTimes } from "./lanes";
import { lostOf, RegionHandles, RegionWave, trimsOf, type Region } from "./regions";
import { Shades } from "./shades";
import { Take } from "./takes";

export type TimelineProps = { page: Page };

/** The drags the chosen clips share: a move of all of them, and a resize of their ends or their starts. */
const [MOVE, RESIZE, RESIZE_START] = ["move", "resize", "resize-start"];
/** How wide a clip's edges are to the pointer, in points. */
const EDGE = 6;
/** What the pointer over a clip that does not play reads. */
const UNPLAYED = "Not played: the instrument makes its own notes";

/**
 * The tracks in time on the plane the ruler shares: their clips and regions, the lanes of those
 * that show them, the grid, the playhead, and the foot beside the returns and the master. A press
 * on empty space lets go of the chosen, a double click on a track adds a clip, a drag chooses what
 * it covers, a right press opens its menu.
 */
export const Timeline = ({ page }: TimelineProps) => {
  const { song, rows, tracks, end, height, low } = page;
  const marquee = live.drag(PLANE);
  const chosen = new Set(song.memory.selected);
  // The clip a hint offers to start with: the first instrument track, while the song has no clip.
  const hinted = tracks.some((each) => each.children.some((child) => child.tool === "timeline")) ? undefined : tracks.find((each) => each.tool === "track")?.name;
  const bounds = boundsOf(page, chosen);
  return (
    <col grow={1} h="full">
      <row grow={1} w="full">
        <box w={GUTTER} h="full" />
        <box key={PLANE} label="Timeline" grow={1} h="full" plane={{ ...TIME, follow: live.playhead }} drag={{ x: true, y: true }}
          onPress={(p) => {
            const row = rows.find((each) => p.y >= each.top && p.y < each.top + each.height);
            if (p.right) return;
            if (p.clicks === 2 && row?.kind === "track") page.edits.clips.add({ x: p.x, y: tracks.indexOf(row.track) });
            else page.edits.clips.clear();
          }}
          onDragEnd={(p) => {
            const [from, to] = [trackY(rows, tracks, p.y), trackY(rows, tracks, p.y + p.dy)];
            page.edits.clips.marquee({ ...p, y: from, dy: to - from });
          }}
          onDrop={(d) => {
            const at = { x: d.x, y: trackAt(rows, tracks, d.y) };
            if (d.parts.length > 0) page.edits.clips.dropNotes(d.parts, at);
            if (d.audio.length > 0) page.edits.clips.dropAudio(d.audio, at);
          }}>
          <box place={[0, 0, end, height + 1]} />
          <Shades page={page} id="timeline" tall={height} />
          <SongGrid runs={song.clock.tempo_map.time_signatures} end={end} height={height} />
          {rows.map((row) =>
            row.kind === "track" ? <TrackTime page={page} row={row} chosen={chosen} bounds={bounds} hint={hinted === row.track.name ? "Double-click to add a clip" : row.track.tool === "tape" ? "Drop an audio file here" : ""} /> : null,
          )}
          {laneGroups(rows).map((lanes) => <LaneTimes page={page} rows={lanes} />)}
          <box place={[marquee.x.min(marquee.x.plus(marquee.dx)), marquee.y.min(marquee.y.plus(marquee.dy)), marquee.dx.abs(), marquee.dy.abs()]} radius="sm" fill="fill" border="border-strong" opacity={marquee.active} />
          <MenuList id="timeline-menu" label="Menu of the timeline" items={emptyMenu(page)} open="menu" />
          <Playhead plane={PLANE} />
        </box>
      </row>
      <box key="timeline-foot" label="Under the tracks" w="full" h={low} border="border" borderSides={[1, 0, 0, 0]} onPress={() => page.edits.clips.clear()} />
      {tracks.length === 0 && <EmptySong page={page} />}
    </col>
  );
};

/** How far the chosen may move as one: the earliest to tick 0, the top one to the first row and the bottom one to the last. */
function boundsOf({ rows, song }: Page, chosen: ReadonlySet<string>) {
  const tops = rows.flatMap((row) => (row.kind === "track" && [...chosen].some((key) => rowOfKey(key) === row.track.name) ? [row.top] : []));
  const starts = [...chosen].flatMap((key) => itemAt(song.children, key)?.state.start ?? []);
  const last = rows.findLast((row) => row.kind === "track")?.top ?? 0;
  return { start: starts.length === 0 ? 0 : Math.min(...starts), up: -Math.min(...tops, last), down: last - Math.max(...tops, 0) };
}
type Bounds = ReturnType<typeof boundsOf>;

/** What a right press on empty space offers: the keys of the clips that need no clip under the pointer. */
const emptyMenu = (page: Page): Item[] => {
  const { song } = page;
  return [
    { text: "Add a clip at the playhead", icon: "plus", keys: song.shortcut("add_clip"), onPress: () => void page.edits.clips.addAtPlayhead() },
    { text: "Paste", icon: "copy", keys: song.shortcut("paste_clips"), ...(song.memory.clipboard ? {} : { disabled: "Nothing is copied" }), onPress: () => void page.edits.clips.paste() },
    { text: "Choose all clips", keys: song.shortcut("select_all_clips"), onPress: () => page.edits.clips.all() },
  ];
};

/** The menu of a clip or a region, which works on all the chosen: the keys of each, and what only a menu does. */
function itemMenu(page: Page, key: string, region: boolean): Item[] {
  const { song } = page;
  const command = (text: string, command: "cut" | "copy" | "duplicate" | "delete", action: string, icon: Item["icon"]): Item => ({ text, icon, keys: song.shortcut(action), onPress: () => void page.edits.clips.command(command) });
  const track = rowOfKey(key);
  const owner = page.tracks.find((each) => each.name === track);
  const why = cannotRender(owner);
  const ofTrack = () => page.song.memory.selected.filter((each) => rowOfKey(each) === track);
  const clip = itemAt(song.children, key);
  return [
    command("Cut", "cut", "cut_clips", undefined),
    command("Copy", "copy", "copy_clips", "copy"),
    command("Duplicate", "duplicate", "duplicate_clips", undefined),
    ...(region
      ? []
      : [
          { text: "Rename", icon: "pencil", onPress: () => (page.song.memory.renamingClip = key) } satisfies Item,
          { text: "Export notes…", onPress: () => void page.song.exportNotes(key, { name: (clip?.tool === "timeline" && clip.state.title) || (owner ? titleOf(owner) : track) }) } satisfies Item,
        ]),
    ...renders(page, track, ofTrack, why),
    { ...command("Delete", "delete", "delete_clips", "trash-2"), danger: true },
  ];
}

/** The grid of the song's bars, which its time signatures place. */
const SongGrid = ({ runs, end, height }: { runs: Runs; end: number; height: number }) => {
  const bars = barStarts(runs, end);
  return <Grid plane={PLANE} end={end} tall={height} bar={BAR} bars={bars} beats={linesIn(runs, 0, bars.at(-1)!, BEAT)} />;
};

/**
 * The clips or the regions of one track on its row: one memo, so an edit of a clip draws its row
 * again and no other. The chosen move, copy and resize as one.
 */
const TrackTime = ({ page, row, chosen, bounds, hint }: { page: Page; row: Row; chosen: ReadonlySet<string>; bounds: Bounds; hint: string }) => {
  const { song } = page;
  const { memory } = song;
  const track = row.track;
  const colour = colourOf(track);
  const tempo = song.clock.tempo_map;
  const here = [...chosen].filter((key) => rowOfKey(key) === track.name);
  const dim = muted(track) || !plays(track) ? DIM : 1;
  // How tall its lanes are under it, while they show: the chosen shade them too.
  const under = page.rows.filter((each) => each.kind !== "track" && each.track === track).reduce((sum, each) => sum + each.height, 0);
  const keys = ["cut_clips", "copy_clips", "duplicate_clips", "delete_clips"].map((action) => song.shortcut(action)).join(" ");
  const quiet = memory.recording !== null || track.children.some((child) => child.tool === "timeline" || child.tool === "region");
  const naming = memory.renamingClip !== null && rowOfKey(memory.renamingClip) === track.name ? memory.renamingClip : null;
  return memo(`time-${track.name}`, [track.children, colour, tempo, row.top, here, here.length > 0 ? bounds : null, here.length > 0 ? chosen.size : 0, memory.snap, dim, under, keys, quiet ? "" : hint, naming], () => (
    <box>
      <box place={[0, row.top, page.end, 1]} border="border" borderSides={[0, 0, 1, 0]} />
      {!quiet && hint !== "" && (
        <box place={[live.plane(PLANE).left.max(0), row.top, 1, 1]}>
          <text at={[12, ROW / 2 - 8]} style="label" color="text-muted">{hint}</text>
        </box>
      )}
      {track.children.flatMap((item) => {
        if (item.tool !== "timeline" && item.tool !== "region") return [];
        const key = `${track.name}/${item.name}`;
        // An audio clip whose file is not there is two bars long, to say why it is silent.
        const length = item.tool === "timeline" ? lengthOf(item.state) : Math.max(lengthAt({ children: [track], clock: { tempo_map: tempo } }, key), lostOf(item) === null ? 1 : 2 * BAR);
        return [<Item page={page} track={track} itemKey={key} name={item.tool === "timeline" ? item.state.title || "" : item.state.asset.replace(/\.[^.]+$/, "")} region={item.tool === "region" ? item : undefined} start={item.state.start} length={length}
          notes={item.tool === "timeline" ? item.state.notes : []} top={row.top} under={under} chosen={chosen.has(key)} bounds={bounds} colour={colour} dim={dim} />];
      })}
      <Take page={page} track={track} top={row.top} />
    </box>
  ));
};

/** Whether the instrument of `track` plays its clips: one that makes its own notes, such as a step sequencer, hears none of them. */
const plays = (track: Track) => {
  const synth = track.tool === "track" ? track.children.find((child) => child.name === "synth") : undefined;
  return synth === undefined || hearsNotes(String(synth.tool));
};

type ItemProps = {
  page: Page;
  track: Track;
  itemKey: string;
  name: string;
  region?: Region;
  start: number;
  length: number;
  notes: readonly { start: number; length: number; pitch: number }[];
  top: number;
  under: number;
  chosen: boolean;
  bounds: Bounds;
  colour: Color;
  dim: number;
};

/**
 * A clip or a region where it plays: a press chooses it (Shift or Cmd adds it), a double click
 * opens a clip's notes, a drag moves the chosen on the snap (Cmd: free, Alt: a copy, the original
 * staying), its edges resize the chosen clips, a right press opens its menu.
 */
const Item = ({ page, track, itemKey: key, name, region, start, length, notes, top, under, chosen, bounds, colour, dim }: ItemProps) => {
  const { memory } = page.song;
  const moved = live.drag(chosen ? MOVE : `clip-${key}`);
  const [end, begin] = [live.drag(RESIZE), live.drag(RESIZE_START)];
  // A region trims while it is chosen alone, and shows its handles.
  const resizes = chosen && (!region || page.song.memory.selected.length === 1);
  const runs = page.song.clock.tempo_map.time_signatures;
  const shortest = stepAt(runs, start, memory.snap);
  const trims = region && trimsOf(page, key, region, start, length, memory.snap > 0 ? shortest : 1);
  const x = resizes ? moved.dx.plus(begin.dx).plus(start) : moved.dx.plus(start);
  const width = resizes ? end.dx.minus(begin.dx).plus(length) : Live.of(length);
  const y = moved.dy.plus(top);
  const called = region ? `Audio clip ${words(name)}` : name ? `Clip ${name}` : `Clip at ${placeOf(runs, start).slice(0, 2).join(".")} on ${titleOf(track)}`;
  return (
    <box>
      {chosen && <box place={[start, top, length, 1]} inset={[0, INSET, 0, INSET]} radius="md" fill="fill" border="border-strong" opacity={moved.alt.times(moved.active).times(dim)} />}
      {chosen && under > 0 && <box place={[x, moved.dy.plus(top + 1), width, under]} fill="selected" />}
      <Clip id={`clip-${key}`} label={called} chosen={chosen} place={[x, y, width, 1]} opacity={dim} keys={false} menu={itemMenu(page, key, region !== undefined)}
        drag={{
          x: [-(chosen ? bounds.start : start), null],
          y: chosen ? [bounds.up, bounds.down] : [-top, page.rows.findLast((each) => each.kind === "track")!.top - top],
          ...dragGrid(runs, start, memory.snap, [1, top]),
          ...(chosen ? { id: MOVE } : {}),
        }}
        onPress={(p) => page.edits.clips.press(key, p)}
        onMove={(p) => {
          const { rows, tracks, edits } = page;
          if (!memory.selected.includes(key)) edits.clips.choose([key]);
          edits.clips.move({ dx: p.dx, dy: trackAt(rows, tracks, top + p.dy + 0.5) - tracks.indexOf(track), alt: p.alt });
        }}>
        {notes.length > 0 && <Miniature itemKey={key} chosen={chosen} notes={notes} start={start} top={top} length={length} titled={name !== ""} colour={colour} />}
        {memory.renamingClip === key ? (
          <box at={[4, 4]}>
            <TextField id={`clip-name-${key}`} label={`Name of ${called}`} value={name} focus size="sm" w={140}
              onCommit={(text) => (text === null ? (memory.renamingClip = null) : page.edits.clips.rename(key, text))} />
          </box>
        ) : (
          name !== "" && <text at={[8, 6]} style="label" color="text-secondary" truncate>{name}</text>
        )}
        {region && <RegionWave region={region} path={key} x={moved.dx.plus(start)} y={y} length={length} colour={colour} />}
        {dim < 1 && !muted(track) && <Tooltip text={UNPLAYED} />}
        {resizes && [
          <box key={`clip-end-${key}`} label={`End of ${called}`} inset={[null, 0, 0, 0]} w={EDGE}
            drag={{ x: trims ? trims.end.x : [-(length - shortest), null], ...dragGrid(runs, start + length, memory.snap, [1, top]), id: RESIZE }} onDragEnd={trims ? trims.end.onDragEnd : (p) => page.edits.clips.resizeEnd(p)} />,
          <box key={`clip-start-${key}`} label={`Start of ${called}`} inset={[0, 0, null, 0]} w={EDGE}
            drag={{ x: trims ? trims.start.x : [-start, length - shortest], ...dragGrid(runs, start, memory.snap, [1, top]), id: RESIZE_START }} onDragEnd={trims ? trims.start.onDragEnd : (p) => page.edits.clips.resizeStart(p)} />,
        ]}
      </Clip>
      {region && resizes && <RegionHandles page={page} itemKey={key} region={region} words={called} x={x} y={y} width={width} />}
    </box>
  );
};

/**
 * A clip's notes as the old timeline drew them: a bar each, between its lowest and highest pitch
 * and at least an octave, under its title while it has one. Drawn again only when its notes change.
 */
const Miniature = ({ itemKey, chosen, notes, start, top, length, titled, colour }: { itemKey: string; chosen: boolean; notes: ItemProps["notes"]; start: number; top: number; length: number; titled: boolean; colour: Color }) =>
  memo(`notes-${itemKey}`, [notes, start, top, length, titled, colour, chosen], () => {
    const moved = live.drag(chosen ? MOVE : `clip-${itemKey}`);
    const tall = ROW - 2 * INSET;
    const pitches = notes.map((note) => note.pitch);
    const [lowest, highest] = [Math.min(...pitches), Math.max(...pitches)];
    const range = highest - lowest + 1;
    const shown = Math.max(range, 12);
    const step = (ROW - (titled ? 38 : 24)) / shown;
    const thick = Math.min(Math.max(step, 2), 4);
    const bottom = tall - 8 - ((shown - range) / 2) * step;
    const rects = notes.flatMap((note) => [note.start, bottom - (note.pitch - lowest + 0.5) * step - thick / 2, note.length, thick]);
    return <shapes place={[moved.dx.plus(start), moved.dy.plus(top), length, 1]} inset={[0, INSET, 0, INSET]} units={[length, tall]} rects={[{ color: colour, rects }]} />;
  });

/** What the timeline says while the song has no track. */
const EmptySong = ({ page }: { page: Page }) => (
  <box inset={[0, 0, 0, 0]}>
    <EmptyState title="An empty song" text="Add a track to play or record, or ask the agent for one.">
      <row gap={2}>
        <Button id="empty-instrument" label="Add an instrument track" text="Instrument track" icon="plus" onPress={() => page.edits.tracks.add("instrument")} />
        <Button id="empty-audio" label="Add an audio track" text="Audio track" icon="plus" onPress={() => page.edits.tracks.add("audio")} />
      </row>
    </EmptyState>
  </box>
);
