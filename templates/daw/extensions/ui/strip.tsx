import { h, type Child } from "../sdk";
import theme from "../theme.json";
import { muted, titleOf } from "../song/model";
import { panOf, volumeOf, type Page } from "../song/view";
import { Fader, LINE_HEIGHT, UNDER } from "./fader";
import { Knob } from "./knob";
import { Meter, Peak } from "./meter";
import { Toggle } from "./toggle";

/**
 * A track, an audio track, a return, or `"master"`, and how far its fader travels, in points.
 * `full`, as a panel shows it: its pan's name and value, its volume always under its fader, no
 * loudest number, and `children` (an input, Pre fader) over mute and solo, side by side.
 */
export type StripProps = { page: Page; name: string; height: number; full?: boolean; children?: Child };

/** How wide a strip is, and a full one; the shortest its fader is, beside the pan and the switches; what is under the fader. */
export const [STRIP, FULL_STRIP, SHORTEST_FADER, VALUE_LINE] = [96, 128, 128, UNDER + LINE_HEIGHT];
/** The room left of the meter (px 2), a meter, the room after it and after the fader, the column beside it; the loudest number under the meter. */
const [LEFT, METER, GAP, SIDE, PEAK, CELL] = [8, 10, 4, 32, 34, theme.size.cell];
/** Where the value line starts, from the strip's left: after the loudest number. */
const LINE_AT = 40;

/**
 * The channel of a track, a return or the master: its fader over the level of each channel,
 * beside its pan, mute and solo (an audio track's arm too), and under them the loudest it played
 * and its volume, typed with a click. The numbers at rest (0 dB, centre) show only under the
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
    <row key={`strip-${name}`} label={`${title} strip`} role="group" group hoverGroup w={full ? FULL_STRIP : STRIP} h={height + VALUE_LINE} pad={[2, 0]} gap={1}>
      <box w={METER} h={height}>
        <Meter {...meter} label={`Level ${title}`} w={METER} h={height} />
        {!full && (
          <box at={[GAP - LEFT, height + UNDER]} w={PEAK} h={LINE_HEIGHT}>
            <Peak {...meter} label={`Loudest ${title}`} w={PEAK} />
          </box>
        )}
      </box>
      <Fader {...volume} line={full ? undefined : { x: LEFT + METER + GAP - LINE_AT, w: STRIP - LINE_AT - GAP, quiet: true }} />
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
