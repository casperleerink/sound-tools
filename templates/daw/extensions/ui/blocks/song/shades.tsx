import { h, memo } from "../../../sdk";
import { shadedParts } from "../../../song/marks";
import type { Page } from "../../../song/view";

/** Every other part of the song a shade lighter, `tall` high in the units of the plane it is placed in: under the ruler's, the tempo lane's and the timeline's marks. */
export const Shades = ({ page, id, tall }: { page: Page; id: string; tall: number }) => {
  const { clock } = page.song;
  return memo(`shades-${id}`, [clock.sections, clock.tempo_map.time_signatures, page.end, tall], () => (
    <shapes place={[0, 0, page.end, tall]} rects={[{ color: "fill", rects: shadedParts(clock.sections, clock.tempo_map.time_signatures, page.end).flatMap(([from, to]) => [from, 0, to - from, tall]) }]} />
  ));
};
