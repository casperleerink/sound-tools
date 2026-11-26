import { h, live, Live, memo, type Color } from "../../../sdk";
import { lengthAt, lengthOf } from "../../../song/clips";
import { colourOf, muted } from "../../../song/model";
import { BAR, barAt, barStarts, tickOfBar } from "../../../song/time";
import { GUTTER, laneGroups, PLANE, ROW, TIME, type Page, type Row, type Track } from "../../../song/view";
import { Button } from "../../button";
import { EmptyState } from "../empty-state";
import { DIM } from "../song/heads";
import { LaneTimes } from "../song/lanes";
import { RegionWave, shownLength, tint } from "../song/regions";
import { INSET, Playhead } from "../time";
import { AFTER } from "../../../score/bars";
import { chosenBars } from "../../../score/edits";
import { BarMenu, type Score } from "./bar-menu";

/** How much further out than the DAW's tracks the score zooms, so a long piece shows whole: 256 bars of 4/4 in 250 points. */
const ZOOM_OUT = 10;
/** How far apart the two lines of a double bar line are, and how thick the end line is, in points. */
const [DOUBLE, THICK] = [3, 3];

/** The music in time, one line an instrument, with the lines a score has. */
export const Staves = ({ page, score }: { page: Page; score: Score }) => {
  const { song, rows, tracks, height, low } = page;
  const { memory } = song;
  const end = score.end;
  const runs = song.clock.tempo_map.time_signatures;
  const chosen = chosenBars(song);
  return (
    <col grow={1} h="full">
      <row grow={1} w="full">
        <box w={GUTTER} h="full" />
        <box key={PLANE} label="Score" grow={1} h="full" plane={{ x: [0, end + AFTER], y: TIME.y, zoom: [TIME.zoom[0] / ZOOM_OUT, TIME.zoom[1]], follow: live.playhead }}
          onPress={(p) => {
            if (p.right) return score.edits.chooseForMenu(barAt(runs, Math.min(Math.max(p.x, 0), end - 1)).bar);
            const row = rows.find((each) => p.y >= each.top && p.y < each.top + each.height);
            if (row?.kind === "track") score.edits.openMusic(row.track.name, p.x);
          }}>
          <box place={[0, 0, end, height + 1]} />
          {chosen && <box place={[tickOfBar(runs, chosen.first), 0, tickOfBar(runs, chosen.last + 1) - tickOfBar(runs, chosen.first), height]} fill="selected" inert />}
          {rows.map((row) => (row.kind === "track" ? <Staff page={page} row={row} end={end} /> : null))}
          {laneGroups(rows).map((lanes) => <LaneTimes page={page} rows={lanes} />)}
          {memo("score-lines", [song.clock.tempo_map.time_signatures, song.clock.sections, end, height], () => <Lines page={page} end={end} />)}
          <BarMenu page={page} score={score} id="score-menu" />
          <Playhead plane={PLANE} />
        </box>
      </row>
      <box key="score-foot" label="Under the music" w="full" h={low} border="border" borderSides={[1, 0, 0, 0]} />
      {tracks.length === 0 && (
        <box inset={[0, 0, 0, 0]}>
          <EmptyState title="An empty score" text="Add an instrument, or ask the agent for one.">
            <Button id="empty-instrument" label="Add an instrument" text="Instrument" icon="plus" onPress={() => page.edits.tracks.add("instrument")} />
          </EmptyState>
        </box>
      )}
    </col>
  );
};

/** The bar lines down every staff: a double line before each section after the first bar, the end line at the end. */
const Lines = ({ page, end }: { page: Page; end: number }) => {
  const { song, height } = page;
  const starts = barStarts(song.clock.tempo_map.time_signatures, end).filter((tick) => tick < end);
  const sections = new Set(song.clock.sections.map((section) => starts[section.bar - 1]).filter((tick) => tick !== undefined && tick > 0));
  // Zoomed out, every fourth bar line stays and the others show once bars are 12 points apart.
  const lines = (every: boolean) => starts.filter((tick, index) => !sections.has(tick) && (index % 4 === 0) !== every).flatMap((tick) => [tick, 0, tick, height]);
  const tall = height * ROW;
  const line = (x: number, color: Color, w = 1) => <box at={[x, 0]} w={w} h={tall} fill={color} />;
  return (
    <box>
      <shapes place={[0, 0, end, height]} lines={[{ color: "border-strong", width: 1, points: lines(false) }]} />
      <shapes place={[0, 0, end, height]} opacity={live.plane(PLANE).zoom.ge(12 / BAR)} lines={[{ color: "border-strong", width: 1, points: lines(true) }]} />
      {[...sections].map((tick) => (
        <box place={[tick, 0, 0, height]} inert>{line(-DOUBLE - 1, "text-muted")}{line(0, "text-muted")}</box>
      ))}
      <box place={[end, 0, 0, height]} inert>{line(-THICK - DOUBLE - 1, "text-muted")}{line(-THICK, "text-secondary", THICK)}</box>
    </box>
  );
};

/**
 * One instrument's line: the line under it, and the notes of all its clips as a score's are, with
 * no clip around them, from its lowest to its highest pitch and at least an octave; or its audio,
 * the sound of each part as it plays.
 */
const Staff = ({ page, row, end }: { page: Page; row: Row; end: number }) => {
  const track: Track = row.track;
  const colour = colourOf(track);
  const dim = muted(track) ? DIM : 1;
  const { song } = page;
  return memo(`staff-${track.name}`, [track.children, song.clock.tempo_map, colour, dim, row.top, end], () => {
    const tall = ROW - 2 * INSET;
    // A note is drawn as long as it sounds: to the end of its clip at most.
    const notes = track.children.flatMap((child) => {
      if (child.tool !== "timeline") return [];
      const length = lengthOf(child.state);
      return child.state.notes.filter((note) => note.start < length).map((note) => ({ ...note, start: child.state.start + note.start, length: Math.min(note.length, length - note.start) }));
    });
    const pitches = notes.map((note) => note.pitch);
    const [lowest, highest] = [Math.min(...pitches), Math.max(...pitches)];
    const range = highest - lowest + 1;
    const shown = Math.max(range, 12);
    const step = (tall - 16) / shown;
    const thick = Math.min(Math.max(step, 2), 3);
    const bottom = tall - 8 - ((shown - range) / 2) * step;
    const rects = notes.flatMap((note) => [note.start, bottom - (note.pitch - lowest + 0.5) * step - thick / 2, note.length, thick]);
    return (
      <box>
        <box place={[0, row.top, end, 1]} border="border" borderSides={[0, 0, 1, 0]} inert />
        {rects.length > 0 && <shapes key={`notes-${track.name}`} place={[0, row.top, end, 1]} inset={[0, INSET, 0, INSET]} units={[end, tall]} opacity={dim} rects={[{ color: colour, rects }]} />}
        {track.children.map((child) => {
          if (child.tool !== "region") return null;
          const path = `${track.name}/${child.name}`;
          const [start, length] = [child.state.start, shownLength(child, lengthAt(song, path))];
          return (
            <box place={[start, row.top, length, 1]} inset={[0, INSET, 0, INSET]} radius="sm" fill={tint(colour, 12)} opacity={dim} clip inert>
              <RegionWave region={child} path={path} x={Live.of(start)} y={Live.of(row.top)} length={length} colour={colour} top={0} />
            </box>
          );
        })}
      </box>
    );
  });
};
