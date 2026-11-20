import { h, memo } from "../../../sdk";
import { barStarts, SNAPS } from "../../../song/time";
import { GUTTER, HEAD, PLANE, RULER, TEMPO, type Page } from "../../../song/view";
import { Select } from "../../select";

export type RulerProps = { page: Page };

/**
 * The band over the tracks: the snap in its corner, the bars of the song with its sections,
 * tempo and time signature changes and the repeat, and the tempo lane under it while it shows.
 */
export const Ruler = ({ page }: RulerProps) => {
  const { song, end } = page;
  const runs = song.clock.tempo_map.time_signatures;
  const bars = memo("ruler-bars", [runs, end], () => (
    <box key="ruler" label="Ruler" grow={1} h={RULER} plane={{ x: PLANE }}>
      {barStarts(runs, end).map((tick, index) => (
        <box place={[tick, 0, 1, RULER]} border="border-strong" borderSides={[0, 0, 0, 1]}>
          <text at={[4, 6]} style="label" color="text-muted">{index + 1}</text>
        </box>
      ))}
    </box>
  ));
  return (
    <col w="full">
      <row w="full" h={RULER} border="border" borderSides={[0, 0, 1, 0]}>
        <row w={HEAD} h={RULER} px={2} gap={2} align="center" border="border" borderSides={[0, 1, 0, 0]}>
          <text style="label" color="text-secondary">Snap</text>
          <Select id="snap" label="Snap" w={72} options={SNAPS.map(({ label, ticks }) => ({ value: ticks, text: label }))} value={song.memory.snap} onChange={(ticks) => (song.memory.snap = ticks)} />
        </row>
        <box w={GUTTER} />
        {bars}
      </row>
      {song.memory.tempoLane && <row w="full" h={TEMPO} border="border" borderSides={[0, 0, 1, 0]} />}
    </col>
  );
};
