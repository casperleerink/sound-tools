// What a region of an audio track keeps of its file: where it starts and ends, its fades and its
// gain, each change one undo step. Its handles' plane: x in ticks of the song; the gain's y in dB down.
import type { FilledStateOfTool, Pointer } from "../sdk";
import { itemAt, rowOfKey, type Song } from "./model";
import { secondsBetween, ticksAcross } from "./time";

type Region = FilledStateOfTool<"region">;
/** The gain a region's handle and keys move it in, in dB. */
export const GAIN = [-48, 24] as const;
const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);
/** A gain a tenth of a dB apart, in its range; one under it counts from its bottom. */
const gainOf = (db: number) => clamp(Math.round(db * 10) / 10, GAIN[0], GAIN[1]);

/** What a view does to the regions: `seconds` is how long the region plays, as its facts say. */
export const regionEdits = (song: Song) => {
  const changes = () => song.clock.tempo_map.tempo_changes;
  const edit = (label: string, keys: readonly string[], change: (region: Region) => void) =>
    song.update(label, ({ children }) => {
      for (const key of keys) {
        const owner = children.find((child) => child.name === rowOfKey(key));
        const region = owner?.tool === "tape" ? owner.children.find((child) => child.name === key.slice(key.indexOf("/") + 1)) : undefined;
        if (region?.tool === "region") change(region.state);
      }
    });
  /** The milliseconds a fade `ticks` long from the start of `region` lasts, at most what plays less the other fade. */
  const fadeMs = (region: Region, ticks: number, seconds: number, other: number) =>
    clamp(Math.round(secondsBetween(changes(), region.start, region.start + Math.max(ticks, 0)) * 1000), 0, Math.max(seconds * 1000 - other, 0));
  const fadeTicks = (region: Region, ms: number) => ticksAcross(changes(), region.start, ms / 1000);
  return {
    /** A drag of its left edge by `dx`: it starts later or earlier, what it plays of its file with it. */
    trimStart(key: string, p: Pick<Pointer, "dx">) {
      edit("Trim a region", [key], (region) => {
        const ticks = Math.max(Math.round(p.dx), -region.start);
        const moved = secondsBetween(changes(), region.start, region.start + ticks);
        region.start += ticks;
        region.file_start_seconds = Math.max(region.file_start_seconds + moved, 0);
      });
    },
    /** A drag of its right edge by `dx`: it ends earlier or later, never past the end of its file. */
    trimEnd(key: string, p: Pick<Pointer, "dx">, seconds: number) {
      const region = itemAt(song.children, key);
      // Its whole file plays already: there is nothing after its end.
      if (region?.tool !== "region" || seconds <= 0 || (region.state.file_end_seconds === null && p.dx >= 0)) return;
      edit("Trim a region", [key], (state) => {
        const end = state.start + ticksAcross(changes(), state.start, seconds);
        state.file_end_seconds = state.file_start_seconds + Math.max(seconds + secondsBetween(changes(), end, end + p.dx), 0.05);
      });
    },
    fadeIn(key: string, p: Pick<Pointer, "dx">, seconds: number) {
      edit("Change fade in", [key], (region) => (region.fade_in_ms = fadeMs(region, fadeTicks(region, region.fade_in_ms) + p.dx, seconds, region.fade_out_ms)));
    },
    fadeOut(key: string, p: Pick<Pointer, "dx">, seconds: number) {
      edit("Change fade out", [key], (region) => (region.fade_out_ms = fadeMs(region, fadeTicks(region, region.fade_out_ms) - p.dx, seconds, region.fade_in_ms)));
    },
    /** A drag of its gain handle `dy` dB down. */
    gain(key: string, p: Pick<Pointer, "dy">) {
      edit("Change gain", [key], (region) => (region.gain_db = gainOf(Math.max(region.gain_db, GAIN[0]) - p.dy)));
    },
    /** Alt-up and alt-down: the chosen regions a dB louder or softer; whether one is chosen. */
    step(by: -1 | 1): boolean {
      const keys = song.memory.selected.filter((key) => itemAt(song.children, key)?.tool === "region");
      if (keys.length === 0) return false;
      edit("Change gain", keys, (region) => (region.gain_db = gainOf(Math.max(region.gain_db, GAIN[0]) + by)));
      return true;
    },
  };
};
