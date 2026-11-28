// What a region of an audio track keeps of its file: where it starts and ends, its fades and its
// gain, each change one undo step. Its handles' plane: x in ticks of the song; the gain's y in dB down.
import { tempoSteps, type FilledStateOfTool, type Pointer } from "../sdk";
import { itemAt, rowOfKey, type Song } from "./model";
import { secondsBetween, ticksAcross, ticksBefore } from "./time";

type Region = FilledStateOfTool<"region">;
/** The gain a region's handle and keys move it in, in dB. */
export const GAIN = [-48, 24] as const;
const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);
/** A gain a tenth of a dB apart, in its range; one under it counts from its bottom. */
const gainOf = (db: number) => clamp(Math.round(db * 10) / 10, GAIN[0], GAIN[1]);

/** Where in its file a region ends, in seconds: its end, else the end of its file; none while its file is not known. */
export const fileEndOf = ({ state, facts }: Extract<NonNullable<ReturnType<typeof itemAt>>, { tool: "region" }>) => state.file_end_seconds ?? facts.file.seconds ?? null;

/** What a view does to the regions: `seconds` is how long the region plays, as its facts say. */
export const regionEdits = (song: Song) => {
  const changes = () => tempoSteps(song.clock.tempo_map);
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
  const edits = {
    /** A drag of its left edge by `dx`: it starts later or earlier, what it plays of its file with it, no earlier than its file begins. */
    trimStart(key: string, p: Pick<Pointer, "dx">) {
      edit("Trim a region", [key], (region) => {
        const ticks = Math.max(Math.round(p.dx), -Math.min(region.start, Math.floor(ticksBefore(changes(), region.start, region.file_start_seconds))));
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
    /** A fade out is measured back from the end, in the tempo there. */
    fadeOut(key: string, p: Pick<Pointer, "dx">, seconds: number) {
      edit("Change fade out", [key], (region) => {
        const end = region.start + ticksAcross(changes(), region.start, seconds);
        const ticks = Math.min(Math.max(ticksBefore(changes(), end, region.fade_out_ms / 1000) - p.dx, 0), end);
        region.fade_out_ms = clamp(Math.round(secondsBetween(changes(), end - ticks, end) * 1000), 0, Math.max(seconds * 1000 - region.fade_in_ms, 0));
      });
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
    /** A knob of its card: its gain in dB. */
    setGain(key: string, db: number) {
      edit("Change gain", [key], (region) => (region.gain_db = gainOf(db)));
    },
    /** A knob of its card: a fade `ms` long, at most what plays (`seconds`, when known) less the other fade. */
    setFade(key: string, which: "in" | "out", ms: number, seconds: number | null) {
      edit(which === "in" ? "Change fade in" : "Change fade out", [key], (region) => {
        const other = which === "in" ? region.fade_out_ms : region.fade_in_ms;
        const fade = clamp(Math.round(ms), 0, seconds === null ? Infinity : Math.max(seconds * 1000 - other, 0));
        if (which === "in") region.fade_in_ms = fade;
        else region.fade_out_ms = fade;
      });
    },
    /** A knob of its card: where in its file it starts, in seconds, short of its end. What it plays stays where it was in time: it starts that much later or earlier. */
    setStart(key: string, at: number) {
      const region = itemAt(song.children, key);
      if (region?.tool !== "region") return;
      const { start, file_start_seconds } = region.state;
      const to = clamp(at, 0, (fileEndOf(region) ?? Infinity) - 0.05);
      const later = to - file_start_seconds;
      edits.trimStart(key, { dx: later >= 0 ? ticksAcross(changes(), start, later) : -ticksBefore(changes(), start, -later) });
    },
    /** A knob of its card: where in its file it ends, in seconds. */
    setEnd(key: string, at: number) {
      edit("Trim a region", [key], (region) => (region.file_end_seconds = Math.max(at, region.file_start_seconds + 0.05)));
    },
  };
  return edits;
};
