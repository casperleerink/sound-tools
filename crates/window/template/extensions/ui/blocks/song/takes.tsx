// What records on a track: the take that runs, growing with the playhead, and the input an armed
// track hears. The view runs as a take begins and ends, never as it grows.
import { h, live } from "../../../sdk";
import { colourOf, titleOf } from "../../../song/model";
import { BAR } from "../../../song/time";
import type { Page, Track } from "../../../song/view";
import { liveLevel } from "../../value";
import { INSET } from "../time";
import { tint } from "./regions";

/**
 * The take that runs on `track`, from where it began to the playhead, in the row at `top`: a red border, its sound so far on an audio track, else its notes, at
 * the pitches of the track's clips.
 */
export const Take = ({ page, track, top }: { page: Page; track: Track; top: number }) => {
  const recorder = track.children.find((child) => child.tool === "recorder");
  const from = recorder?.tool === "recorder" ? recorder.facts.taking?.from : null;
  if (from === null || from === undefined) return null;
  const path = `${track.name}/rec`;
  const colour = colourOf(track);
  // What it draws into is as long as the song: the take box shows as much as has been recorded.
  const span = Math.max(page.end - from, BAR);
  return (
    <box key={`take-${track.name}`} label={`Recording on ${titleOf(track)}`} place={[from, top, live.playhead.minus(from).max(1), 1]} inset={[0, INSET, 0, INSET]} radius="md" fill="surface" border="danger" clip inert>
      <text at={[8, 4]} style="label" color="danger">Recording</text>
      {track.tool === "tape" ? (
        <shapes place={[from, top, span, 1]} inset={[0, INSET + 22, 0, INSET + 1]} units={[span, 1]} rects={[{ color: tint(colour, 60), rects: live.feed("take", path) }]} />
      ) : (
        <shapes place={[from, top, span, 1]} inset={[0, INSET + 22, 0, INSET + 1]} units={[span, pitchesOf(track)]} rects={[{ color: colour, rects: live.feed("notes", path) }]} />
      )}
    </box>
  );
};

/** The pitches a take's notes draw between, top and bottom: those of the track's clips, at least two octaves. */
function pitchesOf(track: Track): [number, number] {
  const pitches = track.children.flatMap((child) => (child.tool === "timeline" ? child.state.notes.map((note) => note.pitch) : []));
  const [low, high] = pitches.length === 0 ? [60, 60] : [Math.min(...pitches), Math.max(...pitches)];
  const [middle, half] = [(low + high) / 2, Math.max((high - low) / 2 + 2, 12)];
  return [Math.ceil(middle + half + 1), Math.floor(middle - half)];
}

/** The level the input of an armed track hears, beside its name. */
export const InputLevel = ({ path, title }: { path: string; title: string }) => {
  const level = liveLevel(live.feed("input", path));
  return (
    <box key={`input-${path}`} label={`Input level of ${title}`} role="meter" value={live.feed("input", path).decibels()} w={36} h={4} radius="full" fill="well" clip>
      <box at={[0, 0]} w={level.fall(0.6).times(36)} h={4} fill="sound" />
    </box>
  );
};
