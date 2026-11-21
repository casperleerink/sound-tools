import { h, type Child } from "../sdk";
import theme from "../theme.json";
import { muted, titleOf } from "../song/model";
import { panOf, volumeOf, type Page } from "../song/view";
import { Fader, LINE_HEIGHT, UNDER } from "./fader";
import { Knob } from "./knob";
import { Peak } from "./meter";
import { Toggle } from "./toggle";

/**
 * A track, an audio track, a return, or `"master"`, and how far its fader travels, in points.
 * `full`, as a panel shows it: its pan's name and value, its volume always under its fader, no
 * loudest number, and `children` (an input, Pre fader) over mute and solo, side by side.
 */
export type StripProps = { page: Page; name: string; height: number; full?: boolean; children?: Child };

/** How wide a strip is, and a full one; the shortest its fader is, beside the pan and the switches. */
export const [STRIP, FULL_STRIP, SHORTEST_FADER] = [80, 114, 128];
/** The room left of the fader (px 2), the column beside it, the cell beside it in a full strip. */
const [LEFT, SIDE, CELL] = [8, 32, theme.size.cell];
/** Under the fader: its value line, as wide as "−12.5 dB", and under that the loudest number, as wide as "−12.5". */
const [LINE, LOUDEST, PEAK] = [52, 16, 34];
/** What is under the fader of a strip: its value line and the loudest number. */
export const UNDER_FADER = UNDER + LINE_HEIGHT + LOUDEST;

/**
 * The channel of a track, a return or the master: its fader over the level of each channel,
 * beside its pan, mute and solo (an audio track's arm too), and under them its volume, typed with
 * a click, and under that the loudest it played. The numbers at rest (0 dB, centre) show only under the
 * pointer, but in a full strip. Its controls are one stop of Tab, which the arrows walk.
 */
export const Strip = ({ page, name, height, full = false, children }: StripProps) => {
  const { song, edits } = page;
  const each = page.strips.find((strip) => strip.name === name);
  const title = each ? titleOf(each) : "Master";
  const master = name === "master" ? page.self.children.find((child) => child.name === "master") : undefined;
  const isMuted = each ? muted(each) : master?.tool === "fader" && master.state.mute === true;
  const meter = { id: `meter-${name}`, path: name === "master" ? "master" : `${name}/mix` };
  const volume = { ...volumeOf(page, name), id: `fader-${name}`, label: `Volume ${title}`, height };
  const pan = { ...panOf(page, name), id: `pan-${name}`, label: `Pan ${title}`, bipolar: true };
  const toggles = [
    <Toggle id={`strip-mute-${name}`} label={`Mute ${title}`} text="M" on={isMuted} color="warning" tip="Mute" onToggle={() => edits.tracks.mute(name)} />,
    each && each.tool !== "bus" && (
      <Toggle id={`strip-solo-${name}`} label={`Solo ${title}`} text="S" on={song.state.solo.includes(name)} color="hot" tip="Solo" onToggle={() => edits.tracks.solo(name)} />
    ),
    each?.tool === "tape" && (
      <Toggle id={`strip-arm-${name}`} label={`Arm ${title}`} on={song.memory.armed.includes(name)} color="danger" dot tip="Arm" onToggle={() => edits.tracks.arm(name)} />
    ),
  ];
  return (
    <row key={`strip-${name}`} label={`${title} strip`} role="group" group hoverGroup w={full ? FULL_STRIP : STRIP} h={height + UNDER_FADER - (full ? LOUDEST : 0)} pad={[2, 0]} justify="between">
      <Fader {...volume} meter={{ ...meter, label: `Level ${title}` }} line={{ x: LEFT, w: LINE, quiet: !full }} />
      {!full && (
        <box at={[(LINE - PEAK) / 2, height + UNDER + LINE_HEIGHT]} w={PEAK} h={LOUDEST}>
          <Peak {...meter} label={`Loudest ${title}`} w={PEAK} />
        </box>
      )}
      {full ? (
        <col w={CELL} h={height} gap={1} align="center">
          <Knob {...pan} text="Pan" />
          <box grow={1} />
          {children}
          <row gap={1}>{toggles}</row>
        </col>
      ) : (
        <col w={SIDE} h={height} gap={1} align="center">
          <Knob {...pan} small={{ w: SIDE + 8, quiet: true }} />
          <box grow={1} />
          {toggles}
        </col>
      )}
    </row>
  );
};
