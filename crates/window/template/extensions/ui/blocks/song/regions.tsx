// An audio clip on the timeline: its sound as it plays, its gain and fades in it, or the words of
// a file that is not there; its trims; and, chosen alone, the handles of its fades and its gain.
// The window draws the sound and moves the handles; the view runs for none of it.
import { h, live, type Color, type Live, type Pointer } from "../../../sdk";
import { GAIN } from "../../../song/regions";
import { BEAT, secondsBetween, ticksAcross } from "../../../song/time";
import { ROW, type Page, type Track } from "../../../song/view";

/** An audio clip of a track, with its facts. */
export type Region = Extract<Track["children"][number], { tool: "region" }>;

/** Where its sound starts under its name, in points. */
const WAVE = 22;
/** The gain handle's whole range is 200 points of drag: dB a row. */
const DB_PER_ROW = (ROW * (GAIN[1] - GAIN[0])) / 200;

/** `colour` at `percent` of its opacity: a token or `#rrggbb`. */
export const tint = (colour: Color, percent: number): Color =>
  colour.startsWith("#") ? `#${colour.slice(1, 7)}${Math.round(percent * 2.55).toString(16).padStart(2, "0")}` : (`${colour}/${percent}` as Color);

/** Why its file does not play, in words; null while it plays or is read. */
export const lostOf = ({ state, facts }: Region) =>
  facts.file.state === "missing" ? `${state.asset} is missing` : facts.file.state === "does_not_play" ? `${state.asset} does not play` : null;

/**
 * Inside the box of `region` at `path`, under its name: its sound as it plays from `x`, the
 * box's top `y`, `length` ticks and `tall` rows, rising from the foot. A long file reads only what
 * the plane shows; a trim cuts it, as the box clips it.
 */
export const RegionWave = ({ region, path, x, y, length, tall, colour }: { region: Region; path: string; x: Live; y: Live; length: number; tall: number; colour: Color }) => {
  const lost = lostOf(region);
  return lost === null ? (
    <shapes key={`wave-${path}`} place={[x, y.plus(WAVE / ROW), length, tall - (WAVE + 1) / ROW]} units={[length, 1]}
      rects={[{ color: tint(colour, 60), rects: live.feed("wave", path) }]} />
  ) : (
    <row label={lost} at={[8, 28]} gap={1} align="center">
      <icon name="triangle-alert" size={12} color="danger" />
      <text style="label" color="danger">{lost}</text>
    </row>
  );
};

/**
 * How far the edges of `region` trim, which starts at `start` and is `length` ticks: its start no
 * earlier than its file begins nor tick 0, its end no later than its file ends; each at least a
 * step of the snap long. A drag of an edge is one undo step.
 */
export function trimsOf(page: Page, key: string, { state, facts }: Region, start: number, length: number, step: number) {
  const changes = page.song.clock.tempo_map.tempo_changes;
  const earlier = Math.min(start, Math.round(ticksAcross(changes, start, state.file_start_seconds)));
  const rest = state.file_end_seconds === null ? 0 : Math.max((facts.file.seconds ?? 0) - state.file_end_seconds, 0);
  const later = Math.round(ticksAcross(changes, start + length, rest));
  const shortest = Math.min(step, length);
  return {
    start: { x: [-earlier, length - shortest] as [number, number], onDragEnd: (p: Pointer) => page.edits.regions.trimStart(key, p) },
    end: { x: [-(length - shortest), later] as [number, number], onDragEnd: (p: Pointer) => page.edits.regions.trimEnd(key, p, facts.seconds ?? 0) },
  };
}

type HandlesProps = { page: Page; itemKey: string; region: Region; words: string; x: Live; y: Live; width: Live; tall: number };

/**
 * The handles of `region` chosen alone, on its top edge: a fade at each end with its line down to the
 * corner and a shade over what it turns down, the gain in the middle. Each plays what it makes
 * while it drags, so the sound and the wave follow the hand, shows it in a pill above, and is one undo
 * step when let go.
 */
export const RegionHandles = ({ page, itemKey: key, region, words, x, y, width, tall }: HandlesProps) => {
  const { state, facts } = region;
  const seconds = facts.seconds ?? 0;
  const changes = page.song.clock.tempo_map.tempo_changes;
  // A fade is a time, drawn in ticks at the tempo where the clip starts.
  const msPerTick = (secondsBetween(changes, state.start, state.start + BEAT) * 1000) / BEAT;
  const [into, out] = [state.fade_in_ms / msPerTick, state.fade_out_ms / msPerTick];
  const [mostIn, mostOut] = [Math.max(seconds * 1000 - state.fade_out_ms, 0), Math.max(seconds * 1000 - state.fade_in_ms, 0)];
  const [fadeIn, fadeOut, gain] = [`fade-in-${key}`, `fade-out-${key}`, `gain-${key}`];
  const [fadingIn, fadingOut, gaining] = [live.drag(fadeIn), live.drag(fadeOut), live.drag(gain)];
  const inMs = fadingIn.dx.plus(into).times(msPerTick).max(0).min(mostIn);
  const outMs = fadingOut.dx.negate().plus(out).times(msPerTick).max(0).min(mostOut);
  const db = gaining.dy.times(-DB_PER_ROW).plus(Math.max(state.gain_db, GAIN[0])).max(GAIN[0]).min(GAIN[1]);
  const [inX, outX] = [x.plus(inMs.over(msPerTick)), x.plus(width).minus(outMs.over(msPerTick))];
  const shown = (ms: number, held: Live) => held.max(ms > 0 ? 1 : 0);
  const fade = (from: Live, across: Live, line: number[], shade: number[], ms: number, held: Live) => (
    <shapes place={[from, y, across, tall]} units={[1, 1]} opacity={shown(ms, held)}
      lines={[{ color: "well/60", width: 0.5, points: shade, fill: "well/60" }, { color: "text", width: 1.5, points: line }]} />
  );
  const pill = (title: string, value: Live, unit: "ms" | "db", held: Live) => (
    <row at={[8, -30]} h={22} px={2} gap={2} align="center" radius="sm" fill="background" border="border-strong" opacity={held}>
      <text style="label" color="text-secondary">{title}</text>
      <text style="number" color="text" w={56} align="end" live={value} unit={unit} />
    </row>
  );
  return (
    <box>
      {fade(x, inX.minus(x), [0, 1, 1, 0], [0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1], state.fade_in_ms, fadingIn.active)}
      {fade(outX, x.plus(width).minus(outX), [0, 0, 1, 1], [0, 0, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0], state.fade_out_ms, fadingOut.active)}
      <box key={fadeIn} label={`Fade in of ${words}`} role="slider" value={state.fade_in_ms} place={[inX, y]} w={11} h={11} radius="full" fill="text" border="well"
        drag={{ x: [-into, mostIn / msPerTick - into], plays: { [`${key}.fade_in_ms`]: inMs } }} onDragEnd={(p) => page.edits.regions.fadeIn(key, p, seconds)}>
        {pill("Fade in", inMs, "ms", fadingIn.active)}
      </box>
      <box key={fadeOut} label={`Fade out of ${words}`} role="slider" value={state.fade_out_ms} place={[outX, y]} w={11} h={11} radius="full" fill="text" border="well"
        drag={{ x: [-(mostOut / msPerTick - out), out], plays: { [`${key}.fade_out_ms`]: outMs } }} onDragEnd={(p) => page.edits.regions.fadeOut(key, p, seconds)}>
        {pill("Fade out", outMs, "ms", fadingOut.active)}
      </box>
      <box key={gain} label={`Gain of ${words}`} role="slider" value={state.gain_db} place={[x.plus(width.times(0.5)), y]} w={11} h={11} radius="full" fill="well" border="text" borderSides={[2, 2, 2, 2]}
        drag={{ y: [-(GAIN[1] - state.gain_db) / DB_PER_ROW, (Math.max(state.gain_db, GAIN[0]) - GAIN[0]) / DB_PER_ROW], plays: { [`${key}.gain_db`]: db } }}
        onDragEnd={(p) => page.edits.regions.gain(key, { dy: p.dy * DB_PER_ROW })}>
        {pill("Gain", db, "db", gaining.active)}
      </box>
    </box>
  );
};
