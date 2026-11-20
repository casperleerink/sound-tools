import { h, live, memo } from "../../../sdk";
import { pitchName } from "../../../song/notes";
import { HEAD } from "../../../song/view";
import { BLACK_KEY, KEYS, type Roll } from "./roll-view";

/**
 * The keys of a piano beside the roll, down its rows: C to E share 5 rows and F to B 7, so the
 * white keys are as wide as on a piano, and the black keys lie over them. A press plays one, and
 * a key lights while the track sounds it.
 */
export const PianoKeys = ({ roll }: { roll: Roll }) => {
  const { key, top, track } = roll;
  return memo(`keys-${key}`, [top, track.name], () => {
    const sounding = (pitch: number) => <box inset={[0, 0, 0, 0]} radius="sm" fill="sound/60" opacity={live.feed(`sounding.${pitch}`, track.name)} inert />;
    const piano = (pitch: number, place: [number, number, number, number], fill: "fill-strong" | "well") => (
      <box key={`piano-${pitch}`} label={`Play ${pitchName(pitch)}`} role="button" place={place} radius="sm" fill={fill} hover="text-muted" onPress={() => roll.hear(pitch)}>
        {sounding(pitch)}
      </box>
    );
    return (
      <box key={`${key}-keys`} label="Piano keys" w={HEAD} h="full" clip plane={{ y: key }} border="border" borderSides={[0, 1, 0, 0]}>
        {WHITE.map(({ pitch, low, high }) => piano(pitch, [HEAD - 1 - KEYS, 128 - high - top + 0.04, KEYS, high - low - 0.08], "fill-strong"))}
        {PITCHES.filter(isBlack).map((pitch) => piano(pitch, [HEAD - 1 - KEYS, 127 - pitch - top + 0.08, BLACK_KEY, 0.84], "well"))}
        {PITCHES.filter((pitch) => pitch % 12 === 0).map((pitch) => (
          <row place={[0, 127 - pitch - top, HEAD - KEYS - 8, 1]} justify="end" align="center">
            <text style="label" color="text-muted">{pitchName(pitch)}</text>
          </row>
        ))}
      </box>
    );
  });
};

const PITCHES = Array.from({ length: 128 }, (_, pitch) => pitch);
const isBlack = (pitch: number) => [1, 3, 6, 8, 10].includes(pitch % 12);
/** The white keys, and the pitches each spans down the rows. */
const WHITE = Array.from({ length: 11 }, (_, octave) => {
  const low = [0, 2, 4].map((step, index) => ({ step, low: (index * 5) / 3, high: ((index + 1) * 5) / 3 }));
  const high = [5, 7, 9, 11].map((step, index) => ({ step, low: 5 + (index * 7) / 4, high: 5 + ((index + 1) * 7) / 4 }));
  return [...low, ...high].map(({ step, low, high }) => ({ pitch: octave * 12 + step, low: octave * 12 + low, high: Math.min(octave * 12 + high, 128) }));
})
  .flat()
  .filter(({ pitch }) => pitch < 128);
