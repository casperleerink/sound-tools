import { h, hearsNotes, live, Live, memo, type Color, type Num } from "../../../sdk";
import { lengthAt, lengthOf } from "../../../song/clips";
import { colourOf, itemAt, muted, rowOfKey, titleOf, words } from "../../../song/model";
import { cannotRender } from "../../../song/tracks";
import { barStarts, beatRuns, type Runs } from "../../../song/time";
import { ACROSS, GUTTER, laneGroups, latest, PLANE, ROW, trackAt, trackY, ZOOM, type Page, type Row, type Track } from "../../../song/view";
import { Button } from "../../button";
import { keysOf } from "../../kbd";
import { MenuList, type Item } from "../../menu";
import { TextField } from "../../text-field";
import { Tooltip } from "../../tooltip";
import { EmptyState } from "../empty-state";
import { DIM } from "./heads";
import { LaneTimes } from "./lanes";

export type TimelineProps = { page: Page };

/** The drags the chosen clips share: a move of all of them, and a resize of their ends or their starts. */
const [MOVE, RESIZE, RESIZE_START] = ["move", "resize", "resize-start"];
/** How wide a clip's edges are to the pointer, in points, and how far a clip is in from its row's edges. */
const [EDGE, INSET] = [6, 4];
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
  const view = live.plane(PLANE);
  const marquee = live.drag(PLANE);
  const now = () => latest(page);
  const chosen = new Set(song.memory.selected);
  // The clip a hint offers to start with: the first instrument track, while the song has no clip.
  const hinted = tracks.some((each) => each.children.some((child) => child.tool === "timeline")) ? undefined : tracks.find((each) => each.tool === "track")?.name;
  const bounds = boundsOf(page, chosen);
  // What the tempo changes and time signatures are, as text: a memo compares it, not the object a run makes.
  const tempo = JSON.stringify(song.clock.tempo_map);
  return (
    <col grow={1} h="full">
      <row grow={1} w="full">
        <box w={GUTTER} h="full" />
        <box key={PLANE} label="Timeline" grow={1} h="full" plane={{ x: ACROSS, y: ROW, zoom: ZOOM }} drag={{ x: true, y: true }}
          onPress={(p) => {
            const { rows, tracks, edits } = now();
            const row = rows.find((each) => p.y >= each.top && p.y < each.top + each.height);
            if (p.right) return;
            if (p.clicks === 2 && row?.kind === "track") edits.clips.add({ x: p.x, y: tracks.indexOf(row.track) });
            else edits.clips.clear();
          }}
          onDragEnd={(p) => {
            const { rows, tracks, edits } = now();
            const [from, to] = [trackY(rows, tracks, p.y), trackY(rows, tracks, p.y + p.dy)];
            edits.clips.marquee({ ...p, y: from, dy: to - from });
          }}
          onDrop={(d) => {
            const { rows, tracks, edits } = now();
            const at = { x: d.x, y: trackAt(rows, tracks, d.y) };
            if (d.parts.length > 0) edits.clips.dropNotes(d.parts, at);
            if (d.audio.length > 0) edits.clips.dropAudio(d.audio, at);
          }}>
          <box place={[0, 0, end, height + 1]} />
          <Grid runs={song.clock.tempo_map.time_signatures} tempo={tempo} end={end} height={height} />
          {rows.map((row) =>
            row.kind === "track" ? <TrackTime page={page} row={row} tempo={tempo} chosen={chosen} bounds={bounds} hint={hinted === row.track.name ? "Double-click to add a clip" : row.track.tool === "tape" ? "Drop an audio file here" : ""} /> : null,
          )}
          {laneGroups(rows).map((lanes) => <LaneTimes page={page} rows={lanes} />)}
          <box place={[marquee.x.min(marquee.x.plus(marquee.dx)), marquee.y.min(marquee.y.plus(marquee.dy)), marquee.dx.abs(), marquee.dy.abs()]} radius="sm" fill="fill" border="border-strong" opacity={marquee.active} />
          <MenuList id="timeline-menu" label="Menu of the timeline" items={emptyMenu(page)} open="menu" />
        </box>
      </row>
      <box key="timeline-foot" label="Under the tracks" w="full" h={low} border="border" borderSides={[1, 0, 0, 0]} onPress={() => now().edits.clips.clear()} />
      {tracks.length === 0 && <EmptySong page={page} />}
      <box inset={[GUTTER, 0, 0, 0]} clip inert>
        <box key="playhead" inset={[0, 0, null, 0]} w={1} fill="text" at={[live.playhead.minus(view.left).times(view.zoom.times(ACROSS)), 0]} />
      </box>
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
  const now = () => latest(page).edits.clips;
  return [
    { text: "Add a clip at the playhead", icon: "plus", keys: keysOf(song.shortcut("add_clip")), onPress: () => void now().addAtPlayhead() },
    { text: "Paste", icon: "copy", keys: keysOf(song.shortcut("paste_clips")), ...(song.memory.clipboard ? {} : { disabled: "Nothing is copied" }), onPress: () => void now().paste() },
    { text: "Choose all clips", keys: keysOf(song.shortcut("select_all_clips")), onPress: () => now().all() },
  ];
};

/** The menu of a clip or a region, which works on all the chosen: the keys of each, and what only a menu does. */
function itemMenu(page: Page, key: string, region: boolean): Item[] {
  const { song } = page;
  const now = () => latest(page);
  const command = (text: string, command: "cut" | "copy" | "duplicate" | "delete", action: string, icon: Item["icon"]): Item => ({ text, icon, keys: keysOf(song.shortcut(action)), onPress: () => void now().edits.clips.command(command) });
  const track = rowOfKey(key);
  const owner = page.tracks.find((each) => each.name === track);
  const why = cannotRender(owner);
  const ofTrack = () => now().song.memory.selected.filter((each) => rowOfKey(each) === track);
  const clip = itemAt(song.children, key);
  return [
    command("Cut", "cut", "cut_clips", undefined),
    command("Copy", "copy", "copy_clips", "copy"),
    command("Duplicate", "duplicate", "duplicate_clips", undefined),
    ...(region
      ? []
      : [
          { text: "Rename", icon: "pencil", onPress: () => (now().song.memory.renamingClip = key) } satisfies Item,
          { text: "Export notes…", onPress: () => void now().song.exportNotes(key, { name: (clip?.tool === "timeline" && clip.state.title) || (owner ? titleOf(owner) : track) }) } satisfies Item,
        ]),
    ...[true, false].map((tail): Item => ({ text: tail ? "Render to audio track" : "Render without the tail", icon: "audio-lines", ...(why === undefined ? {} : { disabled: why }), onPress: () => void now().edits.tracks.render(track, ofTrack(), tail) })),
    { ...command("Delete", "delete", "delete_clips", "trash-2"), danger: true },
  ];
}

/** The grid: a line at each bar, fewer as the song zooms out, and at each beat zoomed in; the window thins them, the view runs for none of it. */
const Grid = ({ runs, tempo, end, height }: { runs: Runs; tempo: string; end: number; height: number }) =>
  memo("grid", [tempo, end, height], () => {
    const view = live.plane(PLANE);
    const bars = barStarts(runs, end);
    // Bars an odd multiple of 2^k apart show while they are at least 24 points apart.
    const levels = [0, 1, 2, 3, 4].map((level) => ({
      shown: level === 4 ? Live.of(1) : view.zoom.ge(24 / (BAR_POINTS * 2 ** level)),
      ticks: bars.filter((_, bar) => (level === 4 ? bar % 16 === 0 : bar % 2 ** (level + 1) === 2 ** level)),
    }));
    const beats = beatTicks(runs, bars);
    const lines = (ticks: readonly number[]) => ticks.flatMap((tick) => [tick, 0, tick, height]);
    return (
      <box>
        <shapes place={[0, 0, end, height]} opacity={view.zoom.ge(0.5).times(0.5)} lines={[{ color: "border", width: 1, points: lines(beats) }]} />
        {levels.map(({ shown, ticks }) => <shapes place={[0, 0, end, height]} opacity={shown} lines={[{ color: "border", width: 1, points: lines(ticks) }]} />)}
      </box>
    );
  });
/** A bar of 4/4 in points at zoom 1. */
const BAR_POINTS = 96;
/** The ticks of the beats that are not bars. */
function beatTicks(runs: Runs, bars: readonly number[]) {
  const steps = beatRuns(runs);
  const ticks: number[] = [];
  for (let bar = 0; bar < bars.length - 1; bar++) {
    const step = steps.findLast((run) => run.from <= bars[bar]!)?.ticks ?? 960;
    for (let tick = bars[bar]! + step; tick < bars[bar + 1]!; tick += step) ticks.push(tick);
  }
  return ticks;
}

/**
 * The clips or the regions of one track on its row: one memo, so an edit of a clip draws its row
 * again and no other. The chosen move, copy and resize as one.
 */
const TrackTime = ({ page, row, tempo: tempoKey, chosen, bounds, hint }: { page: Page; row: Row; tempo: string; chosen: ReadonlySet<string>; bounds: Bounds; hint: string }) => {
  const { song } = page;
  const { memory } = song;
  const track = row.track;
  const colour = colourOf(track);
  const tempo = song.clock.tempo_map;
  const here = [...chosen].filter((key) => rowOfKey(key) === track.name);
  const renaming = memory.renamingClip !== null && rowOfKey(memory.renamingClip) === track.name ? memory.renamingClip : null;
  const dim = muted(track) || !plays(track) ? DIM : 1;
  // How tall its lanes are under it, while they show: the chosen shade them too.
  const under = page.rows.filter((each) => each.kind !== "track" && each.track === track).reduce((sum, each) => sum + each.height, 0);
  const keys = ["cut_clips", "copy_clips", "duplicate_clips", "delete_clips"].map((action) => song.shortcut(action)).join(" ");
  const quiet = memory.recording !== null || track.children.some((child) => child.tool === "timeline" || child.tool === "region");
  return memo(`time-${track.name}`, [track.children, colour, tempoKey, row.top, here.join(" "), here.length > 0 ? JSON.stringify(bounds) : null, renaming, memory.snap, dim, under, keys, quiet ? "" : hint], () => (
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
        const length = item.tool === "timeline" ? lengthOf(item.state) : Math.max(lengthAt({ children: [track], clock: { tempo_map: tempo } }, key), 1);
        return [<Item page={page} track={track} itemKey={key} name={item.tool === "timeline" ? item.state.title || "" : ""} region={item.tool === "region"} start={item.state.start} length={length}
          notes={item.tool === "timeline" ? item.state.notes : []} top={row.top} under={under} chosen={chosen.has(key)} bounds={bounds} colour={colour} dim={dim} renaming={renaming === key} />];
      })}
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
  region: boolean;
  start: number;
  length: number;
  notes: readonly { start: number; length: number; pitch: number }[];
  top: number;
  under: number;
  chosen: boolean;
  bounds: Bounds;
  colour: Color;
  dim: number;
  renaming: boolean;
};

/**
 * A clip or a region where it plays: a press chooses it (Shift or Cmd adds it), a double click
 * opens a clip's notes, a drag moves the chosen on the snap (Cmd: free, Alt: a copy, the original
 * staying), its edges resize the chosen clips, a right press opens its menu.
 */
const Item = ({ page, track, itemKey: key, name, region, start, length, notes, top, under, chosen, bounds, colour, dim, renaming }: ItemProps) => {
  const { memory } = page.song;
  const now = () => latest(page);
  const moved = live.drag(chosen ? MOVE : `clip-${key}`);
  const [end, begin] = [live.drag(RESIZE), live.drag(RESIZE_START)];
  const resizes = chosen && !region;
  const x = resizes ? moved.dx.plus(begin.dx).plus(start) : moved.dx.plus(start);
  const width = resizes ? end.dx.minus(begin.dx).plus(length) : Live.of(length);
  const y = moved.dy.plus(top + INSET / ROW);
  const step = memory.snap || 1;
  const words = region ? `Audio clip ${wordsOf(key)}` : `Clip ${name || wordsOf(key)}`;
  // A clip's edge is EDGE points wide at any zoom.
  const edge = Live.of(EDGE / ACROSS).over(live.plane(PLANE).zoom);
  const drag = {
    x: [-(chosen ? bounds.start : start), 1e9] as [number, number],
    y: (chosen ? [bounds.up, bounds.down] : [-top, page.rows.findLast((each) => each.kind === "track")!.top - top]) as [number, number],
    snap: [step, 1] as [number, number],
    from: [start, top] as [number, number],
    ...(chosen ? { id: MOVE } : {}),
  };
  return (
    <box>
      <box place={[start, top + INSET / ROW, length, (ROW - 2 * INSET) / ROW]} radius="md" fill="fill" border="border-strong" opacity={moved.alt.times(moved.active).times(dim)} />
      {chosen && under > 0 && <box place={[x, moved.dy.plus(top + 1), width, under]} fill="selected" />}
      <box key={`clip-${key}`} label={words} role="button" checked={chosen} place={[x, y, width, (ROW - 2 * INSET) / ROW]} radius="md" fill="fill" border={chosen ? "text" : "border-strong"} opacity={dim} clip
        drag={drag}
        onPress={(p) => now().edits.clips.press(key, p)}
        onDragEnd={(p) => {
          const { song, rows, tracks, edits } = now();
          if (!song.memory.selected.includes(key)) edits.clips.choose([key]);
          const row = tracks.findIndex((each) => each.name === track.name);
          edits.clips.move({ dx: p.dx, dy: trackAt(rows, tracks, top + p.dy + 0.5) - row, alt: p.alt });
        }}>
        {notes.length > 0 && <Miniature itemKey={key} chosen={chosen} notes={notes} x={moved.dx.plus(start)} y={y} length={length} titled={name !== ""} colour={colour} />}
        {renaming ? (
          <row at={[4, 4]}><TextField id={`clip-name-${key}`} label={`Name of ${words}`} value={name} focus size="sm" w={140} onCommit={(text) => (text === null ? (memory.renamingClip = null) : now().edits.clips.rename(key, text))} /></row>
        ) : (
          name !== "" && <text at={[8, 6]} style="label" color="text-secondary" truncate>{name}</text>
        )}
        {/* Where a region's waveform draws, from the `wave` feed. */}
        {region && <RegionWave />}
        {dim < 1 && !muted(track) && <Tooltip text={UNPLAYED} />}
        {resizes && (
          <box>
            <box key={`clip-end-${key}`} label={`End of ${words}`} place={[x.plus(width).minus(edge), y, edge, (ROW - 2 * INSET) / ROW]}
              drag={{ x: [-(length - (memory.snap || 240)), 1e9], snap: [step, 1], from: [start + length, top], id: RESIZE }} onDragEnd={(p) => now().edits.clips.resizeEnd(p)} />
            <box key={`clip-start-${key}`} label={`Start of ${words}`} place={[x, y, edge, (ROW - 2 * INSET) / ROW]}
              drag={{ x: [-start, length - (memory.snap || 240)], snap: [step, 1], from: [start, top], id: RESIZE_START }} onDragEnd={(p) => now().edits.clips.resizeStart(p)} />
          </box>
        )}
        <MenuList id={`menu-${key}`} label={`Menu of ${words}`} items={itemMenu(page, key, region)} open="menu" w={260} />
      </box>
    </box>
  );
};

/** `track/clip` as words: `t00/c01` is `C01`. */
const wordsOf = (key: string) => words(key.slice(key.indexOf("/") + 1));

/** A region's waveform, once the window gives a `wave` feed of its file; none until then. */
const RegionWave = () => null;

/**
 * A clip's notes as the old timeline drew them: a bar each, between its lowest and highest pitch
 * and at least an octave, under its title while it has one. Drawn again only when its notes change.
 */
const Miniature = ({ itemKey, chosen, notes, x, y, length, titled, colour }: { itemKey: string; chosen: boolean; notes: ItemProps["notes"]; x: Num; y: Num; length: number; titled: boolean; colour: Color }) =>
  memo(`notes-${itemKey}`, [notes, length, titled, colour, chosen], () => {
    const tall = ROW - 2 * INSET;
    const pitches = notes.map((note) => note.pitch);
    const [lowest, highest] = [Math.min(...pitches), Math.max(...pitches)];
    const range = highest - lowest + 1;
    const shown = Math.max(range, 12);
    const step = (ROW - (titled ? 38 : 24)) / shown;
    const thick = Math.min(Math.max(step, 2), 4);
    const bottom = tall - 8 - ((shown - range) / 2) * step;
    const rects = notes.flatMap((note) => [note.start, bottom - (note.pitch - lowest + 0.5) * step - thick / 2, note.length, thick]);
    return <shapes place={[x, y, length, tall / ROW]} units={[length, tall]} rects={[{ color: colour, rects }]} />;
  });

/** What the timeline says while the song has no track. */
const EmptySong = ({ page }: { page: Page }) => (
  <box inset={[0, 0, 0, 0]}>
    <EmptyState title="An empty song" text="Add a track to play or record, or ask the agent for one.">
      <row gap={2}>
        <Button id="empty-instrument" label="Add an instrument track" text="Instrument track" icon="plus" onPress={() => latest(page).edits.tracks.add("instrument")} />
        <Button id="empty-audio" label="Add an audio track" text="Audio track" icon="plus" onPress={() => latest(page).edits.tracks.add("audio")} />
      </row>
    </EmptyState>
  </box>
);
