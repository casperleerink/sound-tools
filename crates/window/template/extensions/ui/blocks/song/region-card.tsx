import { h, live, type Color } from "../../../sdk";
import { colourOf, itemAt, rowOfKey } from "../../../song/model";
import { GAIN } from "../../../song/regions";
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
        display={<Wave path={region} colour={track ? colourOf(track) : "text"} caption={`${shown(from, "seconds")} to ${to === null ? "the end" : shown(to, "seconds")}`} />}
        cells={[
          knob("gain_db", "Gain", { min: GAIN[0], max: GAIN[1], unit: "db", default: 0 }, gain_db, (db) => edits.regions.setGain(region, db)),
          knob("fade_in_ms", "Fade in", { min: 0, max: Math.max(fade - fade_out_ms, 1), unit: "ms", default: 0 }, fade_in_ms, (ms) => edits.regions.setFade(region, "in", ms, seconds)),
          knob("fade_out_ms", "Fade out", { min: 0, max: Math.max(fade - fade_in_ms, 1), unit: "ms", default: 0 }, fade_out_ms, (ms) => edits.regions.setFade(region, "out", ms, seconds)),
        ]}
        more={[
          knob("file_start_seconds", "Start", { min: 0, max: Math.max(to ?? from + 60, 0.05), unit: "seconds", default: 0 }, from, (at) => edits.regions.setStart(region, at)),
          <Cell text="End" value={to === null ? "End of file" : shown(to, "seconds")} />,
        ]}
        expanded={song.memory.regionMore} onExpand={() => (song.memory.regionMore = !song.memory.regionMore)} />
    </Frame>
  );
};

/**
 * The sound of the clip as it plays, with its gain and fades, as a band around the middle in its
 * track's colour. A line fills only below it, so the lower half fills its whole and the well
 * fills back from its line down.
 */
const Wave = ({ path, colour, caption }: { path: string; colour: Color; caption: string }) => {
  const { w, h: tall } = drawingOf(6);
  const half = (units: [number, number], y: number, fill: Color) => (
    <shapes at={[0, y]} w={w} h={tall / 2} units={[1, units]} lines={[{ color: colour, width: 1, points: live.feed("wave", path), fill }]} />
  );
  return (
    <Display columns={6} caption={caption}>
      {half([1, 0], 0, tint(colour, 45))}
      <box at={[0, tall / 2]} w={w} h={tall / 2} fill={tint(colour, 45)} />
      {half([0, 1], tall / 2, "well")}
    </Display>
  );
};
