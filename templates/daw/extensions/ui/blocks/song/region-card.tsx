import { h, live, type Color } from "../../../sdk";
import { lengthAt } from "../../../song/clips";
import { colourOf, itemAt, rowOfKey } from "../../../song/model";
import { fileEndOf, GAIN } from "../../../song/regions";
import type { Page } from "../../../song/view";
import { Cell } from "../../cell";
import { Display, drawingOf } from "../../display";
import { control } from "../../field";
import { Knob } from "../../knob";
import { shown } from "../../value";
import { DeviceCard, Frame } from "../device-card";
import { tint } from "./regions";

/** The audio clip `region` names, `row/name`, chosen alone. */
export type RegionCardProps = { page: Page; region: string };

/** The longest fade a knob reaches while the length of the file is not known, in ms. */
const FADE = 10_000;

/**
 * The card of an audio clip in its track's panel: the file it plays as its title, its wave with
 * the part that plays, and knobs for its gain and fades; behind More, where in the file it starts.
 */
export const RegionCard = ({ page, region }: RegionCardProps) => {
  const item = itemAt(page.self.children, region);
  if (item?.tool !== "region") return null;
  const { song, edits } = page;
  const { asset, gain_db, fade_in_ms, fade_out_ms, file_start_seconds: from, file_end_seconds: to } = item.state;
  const file = asset.slice(asset.lastIndexOf("/") + 1);
  const track = page.tracks.find((each) => each.name === rowOfKey(region));
  // How long it plays, once its end in the file is set; else up to the end of the file, whose length the song does not know.
  const seconds = to === null ? null : to - from;
  const fade = seconds === null ? FADE : seconds * 1000;
  const knob = (field: string, text: string, range: { min: number; max: number; unit: "db" | "ms" | "seconds"; default: number }, value: number, change: (to: number) => void) => (
    <Knob {...control(song.held, `${region}.${field}`, range, value, change)} id={`region-${field}`} label={`${text} of ${file}`} text={text} />
  );
  return (
    <Frame id="slot-region" title={file}>
      <DeviceCard id="region" title={file}
        display={<Wave path={region} length={lengthAt({ children: page.self.children, clock: song.clock }, region)} colour={track ? colourOf(track) : "text"} caption={`${shown(from, "seconds")} to ${to === null ? "the end" : shown(to, "seconds")}`} />}
        cells={[
          knob("gain_db", "Gain", { min: GAIN[0], max: GAIN[1], unit: "db", default: 0 }, gain_db, (db) => edits.regions.setGain(region, db)),
          knob("fade_in_ms", "Fade in", { min: 0, max: Math.max(fade - fade_out_ms, 1), unit: "ms", default: 0 }, fade_in_ms, (ms) => edits.regions.setFade(region, "in", ms, seconds)),
          knob("fade_out_ms", "Fade out", { min: 0, max: Math.max(fade - fade_in_ms, 1), unit: "ms", default: 0 }, fade_out_ms, (ms) => edits.regions.setFade(region, "out", ms, seconds)),
        ]}
        more={[
          knob("file_start_seconds", "Start", { min: 0, max: Math.max(fileEndOf(item) ?? from + 60, 0.05), unit: "seconds", default: 0 }, from, (at) => edits.regions.setStart(region, at)),
          <Cell text="End" value={to === null ? "End of file" : shown(to, "seconds")} />,
        ]}
        expanded={song.memory.regionMore} onExpand={() => (song.memory.regionMore = !song.memory.regionMore)} />
    </Frame>
  );
};

/** The sound of the clip as it plays, `length` ticks, with its gain and fades, as a band around the middle in its track's colour. */
const Wave = ({ path, length, colour, caption }: { path: string; length: number; colour: Color; caption: string }) => {
  const { w, h: tall } = drawingOf(6);
  return (
    <Display columns={6} caption={caption}>
      <shapes at={[0, 0]} w={w} h={tall} units={[Math.max(length, 1), 1]} rects={[{ color: tint(colour, 60), rects: live.feed("wave", path) }]} />
    </Display>
  );
};
