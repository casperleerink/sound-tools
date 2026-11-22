import { h, live, memo, type Color } from "../../../sdk";
import { changeBars } from "../../../song/marks";
import { colourOf, muted } from "../../../song/model";
import { barStarts } from "../../../song/time";
import { GUTTER, laneGroups, PLANE, ROW, TIME, type Page, type Row, type Track } from "../../../song/view";
import { Button } from "../../button";
import { TextField } from "../../text-field";
import { EmptyState } from "../empty-state";
import { DIM } from "../song/heads";
import { LaneTimes } from "../song/lanes";
import { INSET, Playhead } from "../time";
import { AFTER } from "../../../score/bars";
import { BarMenu, type Score } from "./bar-menu";
import { pinned } from "./marks";

/** How far apart the two lines of a double bar line are, and how thick the end line is, in points. */
const [DOUBLE, THICK] = [3, 3];


/**
 * The music in time, one line an instrument, with the lines a score has: a bar line at each bar,
 * a double one where a section ends, the end line, a line between instruments, and the time
 * signature in every staff where it changes, kept at the left edge while it holds. It opens on
 * the whole piece. A press on an instrument's line opens its notes there; a right press opens
 * the bar's menu.
 */
export const Staves = ({ page, score }: { page: Page; score: Score }) => {
  const { song, rows, tracks, height, low } = page;
  const { memory } = song;
  const end = score.end;
  return (
    <col grow={1} h="full">
      <row grow={1} w="full">
        <box w={GUTTER} h="full" />
        <box key={PLANE} label="Score" grow={1} h="full" plane={{ ...TIME, x: [0, end + AFTER], follow: live.playhead }}
          onPress={(p) => {
            if (p.right) return void (memory.rulerMenu = { x: p.x, mark: null });
            const row = rows.find((each) => p.y >= each.top && p.y < each.top + each.height);
            if (row?.kind === "track") score.edits.openMusic(row.track.name, p.x);
          }}>
          <box place={[0, 0, end, height + 1]} />
          {rows.map((row) => (row.kind === "track" ? <Staff page={page} row={row} end={end} /> : null))}
          {laneGroups(rows).map((lanes) => <LaneTimes page={page} rows={lanes} />)}
          {memo("score-lines", [song.clock.tempo_map.time_signatures, song.clock.sections, end, height], () => <Lines page={page} end={end} />)}
          {memo("score-signatures", [song.clock.tempo_map.time_signatures, end, rows, memory.typing], () => <Signatures page={page} score={score} />)}
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
  const single = starts.filter((tick) => !sections.has(tick)).flatMap((tick) => [tick, 0, tick, height]);
  const tall = height * ROW;
  const line = (x: number, color: Color, w = 1) => <box at={[x, 0]} w={w} h={tall} fill={color} />;
  return (
    <box>
      <shapes place={[0, 0, end, height]} lines={[{ color: "border-strong", width: 1, points: single }]} />
      {[...sections].map((tick) => (
        <box place={[tick, 0, 0, height]} inert>{line(-DOUBLE - 1, "text-muted")}{line(0, "text-muted")}</box>
      ))}
      <box place={[end, 0, 0, height]} inert>{line(-THICK - DOUBLE - 1, "text-muted")}{line(-THICK, "text-secondary", THICK)}</box>
    </box>
  );
};

/**
 * One instrument's line: the line under it, and the notes of all its clips as a score's are, with
 * no clip around them, from its lowest to its highest pitch and at least an octave.
 */
const Staff = ({ page, row, end }: { page: Page; row: Row; end: number }) => {
  const track: Track = row.track;
  const colour = colourOf(track);
  const dim = muted(track) ? DIM : 1;
  return memo(`staff-${track.name}`, [track.children, colour, dim, row.top, end], () => {
    const notes = track.children.flatMap((child) => (child.tool === "timeline" ? child.state.notes.map((note) => ({ ...note, start: child.state.start + note.start })) : []));
    const tall = ROW - 2 * INSET;
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
        {notes.length > 0 && <shapes place={[0, row.top, end, 1]} inset={[0, INSET, 0, INSET]} units={[end, tall]} opacity={dim} rects={[{ color: colour, rects }]} />}
      </box>
    );
  });
};

/**
 * The time signature where it changes, its two numbers stacked as a score writes them, in every
 * staff: a press types another there.
 */
const Signatures = ({ page, score }: { page: Page; score: Score }) => {
  const { song, rows } = page;
  const { memory } = song;
  const runs = song.clock.tempo_map.time_signatures;
  const starts = barStarts(runs, score.end);
  const firsts = changeBars(runs);
  const staves = rows.filter((row) => row.kind === "track");
  return (
    <box>
      {firsts.map((bar, index) => {
        const from = starts[bar - 1];
        if (from === undefined || from >= score.end) return null;
        const to = Math.min(starts[(firsts[index + 1] ?? Infinity) - 1] ?? score.end, score.end);
        const [count, unit] = runs[index]!.signature.split("/");
        const label = `Time signature ${runs[index]!.signature} at bar ${bar}`;
        const typing = memory.typing?.kind === "signature" && memory.typing.bar === bar;
        return staves.map((row, at) => (
          <box place={[from, row.top, to - from, 1]} clip={!typing}>
            {typing && at === 0 ? (
              <box at={[4, 8]}>
                <TextField id="signature-field" label={`Time signature at bar ${bar}`} value={runs[index]!.signature} focus size="sm" w={64} onCommit={(text) => score.edits.typed(text)} />
              </box>
            ) : (
              <col key={`signature-${bar}-${row.track.name}`} label={label} role="button" at={[pinned(from, 3), 9]} w={14} pad={[0, 0]} align="center" radius="sm" fill="background" hover="fill"
                onPress={() => (memory.typing = { kind: "signature", bar })}>
                <text style="heading" color="text-secondary">{count}</text>
                <text style="heading" color="text-secondary">{unit}</text>
              </col>
            )}
          </box>
        ));
      })}
      {memory.typing?.kind === "signature" && !firsts.includes(memory.typing.bar) && staves[0] && (
        <box place={[starts[memory.typing.bar - 1] ?? 0, staves[0].top, 0, 0]}>
          <box at={[4, 8]}>
            <TextField id="signature-field" label={`Time signature at bar ${memory.typing.bar}`} value="" placeholder="3/4" focus size="sm" w={64} onCommit={(text) => score.edits.typed(text)} />
          </box>
        </box>
      )}
    </box>
  );
};
