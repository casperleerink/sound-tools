import { h } from "../sdk";
import { muted, titleOf } from "../song/model";
import { panOf, volumeOf, type Page } from "../song/view";
import { Fader, FaderValue } from "./fader";
import { Knob } from "./knob";
import { Meter, Peak } from "./meter";
import { Toggle } from "./toggle";

/** A track, an audio track, a return, or `"master"`, and how far its fader travels, in points. */
export type StripProps = { page: Page; name: string; height: number };

/** How wide a strip is; the shortest its fader is, beside the pan and the switches; its value line. */
export const [STRIP, SHORTEST_FADER, VALUE_LINE] = [96, 128, 24];
const [METER, SIDE] = [10, 32];

/**
 * The channel of a track, a return or the master: its fader over the level of each channel,
 * beside its pan, mute and solo (an audio track's arm too), and under them the loudest it played
 * and its volume, typed with a click. The numbers at rest (0 dB, centre) show only under the
 * pointer. Its controls are one stop of Tab, which the arrows walk.
 */
export const Strip = ({ page, name, height }: StripProps) => {
  const { song, edits } = page;
  const each = page.strips.find((strip) => strip.name === name);
  const title = each ? titleOf(each) : "Master";
  const master = name === "master" ? page.self.children.find((child) => child.name === "master") : undefined;
  const isMuted = each ? muted(each) : master?.tool === "fader" && master.state.mute === true;
  const meter = { id: `meter-${name}`, path: name === "master" ? "master" : `${name}/mix` };
  const volume = { ...volumeOf(page, name), id: `fader-${name}`, label: `Volume ${title}`, height };
  return (
    <col key={`strip-${name}`} label={`${title} strip`} role="group" group hoverGroup w={STRIP} gap={1}>
      <row h={height} gap={1} justify="center">
        <Meter {...meter} label={`Level ${title}`} w={METER} h={height} />
        <Fader {...volume} bare />
        <col w={SIDE} h={height} gap={1} align="center">
          <Knob {...panOf(page, name)} id={`pan-${name}`} label={`Pan ${title}`} bipolar small={{ w: SIDE + 8, quiet: true }} />
          <box grow={1} />
          <Toggle id={`strip-mute-${name}`} label={`Mute ${title}`} text="M" on={isMuted} color="warning" tip="Mute" onToggle={() => edits.tracks.mute(name)} />
          {each && each.tool !== "bus" && (
            <Toggle id={`strip-solo-${name}`} label={`Solo ${title}`} text="S" on={song.state.solo.includes(name)} color="hot" tip="Solo" onToggle={() => edits.tracks.solo(name)} />
          )}
          {each?.tool === "tape" && (
            <Toggle id={`strip-arm-${name}`} label={`Arm ${title}`} on={song.memory.armed.includes(name)} color="danger" dot tip="Arm" onToggle={() => edits.tracks.arm(name)} />
          )}
        </col>
      </row>
      <row h={VALUE_LINE} align="center" justify="between">
        <Peak {...meter} label={`Loudest ${title}`} w={36} />
        <FaderValue {...volume} w={STRIP - 40} quiet />
      </row>
    </col>
  );
};
