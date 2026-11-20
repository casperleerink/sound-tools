import { h, memo } from "../../../sdk";
import { titleOf } from "../../../song/model";
import { controllerName, CONTROLLERS, LANE_NAMES, LANES, type NoteLane } from "../../../song/notes";
import { nearestOf } from "../../../song/time";
import type { Page } from "../../../song/view";
import { GUTTER, HEAD } from "../../../song/view";
import { Button } from "../../button";
import { Dot } from "../../dot";
import { keysOf } from "../../kbd";
import { Menu } from "../../menu";
import { Select } from "../../select";
import { ExpressionLane } from "./expression";
import { PianoKeys } from "./keys";
import { Readout } from "./readout";
import { noteItems, RollPlane } from "./roll";
import { HEADER, LANE, playheadOf, rollOf, type Roll } from "./roll-view";
import { VelocityLane } from "./velocity";

/** The clip `clip`, `row/name`, whose notes it edits; it fills the height the page gives it. */
export type EditorProps = { page: Page; clip: string };

/**
 * The note editor of one clip, under the tracks: a header with the clip's name, a ruler, the
 * chosen note and its buttons; the keys of a piano beside the notes; and a lane under them.
 */
export const Editor = ({ page, clip }: EditorProps) => {
  const roll = rollOf(page, clip);
  if (!roll) return <box w="full" h="full" fill="background" />;
  const lane = page.song.memory.lane;
  return (
    <col w="full" h="full" fill="background">
      <row w="full" h={HEADER} border="border" borderSides={[0, 0, 1, 0]}>
        <Name roll={roll} />
        <box w={GUTTER} />
        <Ruler roll={roll} />
        <Readout roll={roll} />
        <Buttons roll={roll} />
      </row>
      <row grow={1} w="full">
        <PianoKeys roll={roll} />
        <box w={GUTTER} h="full" />
        <RollPlane roll={roll} />
      </row>
      <row w="full" h={LANE} border="border" borderSides={[1, 0, 0, 0]}>
        <col w={HEAD} h="full" px={3} justify="center" border="border" borderSides={[0, 1, 0, 0]}>
          <Select id="editor-lane" label="Lane under the notes" w={HEAD - 24} menuW={220} search options={laneOptions(roll)} value={lane} onChange={(picked) => (page.song.memory.lane = picked)} />
        </col>
        <box w={GUTTER} />
        {lane === "velocity" ? <VelocityLane roll={roll} /> : <ExpressionLane roll={roll} lane={lane} />}
      </row>
    </col>
  );
};

/** The clip's colour and name, then its track's: a press types a new name. */
const Name = ({ roll }: { roll: Roll }) => {
  const title = roll.state.title ?? "";
  const track = titleOf(roll.track);
  return (
    <row key="clip-name" label="Rename the clip" role="button" value={title || track} w={HEAD} h={HEADER} px={3} gap={2} align="center" hover="fill/50" border="border" borderSides={[0, 1, 0, 0]}>
      <Dot color={roll.colour} />
      {title ? <text style="heading" color="text" truncate>{title}</text> : null}
      <text style={title ? "label" : "heading"} color={title ? "text-secondary" : "text"} truncate>{track}</text>
      <row overlay={{ open: "press", side: "bottom", align: "start", gap: -HEADER + 4 }} w={HEAD - 8} h={HEADER - 8} px={2} align="center" radius="md" fill="raised" border="focus" shadow="md">
        <field key="clip-name-typed" label="Name of the clip" value={title} placeholder={track} style="heading" w={HEAD - 24} focus onCommit={(typed) => typed !== null && typed.trim() !== title && roll.page.edits.clips.rename(roll.clip, typed)} />
      </row>
    </row>
  );
};

/** The bars of the song over the notes: a press plays from there, on the snap. */
const Ruler = ({ roll }: { roll: Roll }) => {
  const { key, bars, state, page } = roll;
  const snap = page.song.memory.snap;
  const { at, inside } = playheadOf(roll);
  return (
    <box key={`${key}-ruler`} label="Bars of the clip" grow={1} h={HEADER} clip plane={{ x: key }} onPress={(p) => page.song.clock.jump(state.start + nearestOf(p.x, snap))}>
      {memo(`ruler-${key}`, [roll.runs, state.start, roll.end], () => (
        <box>
          {bars.map(({ tick, bar }) => (
            <box place={[tick, HEADER - 10, 1, 10]} fill="border-strong">
              <text at={[5, -14]} style="label" color="text-muted">{bar}</text>
            </box>
          ))}
        </box>
      ))}
      <box place={[at, HEADER / 2]} w={1} h={HEADER} fill="text" opacity={inside} inert />
    </box>
  );
};

/** Larger or smaller, the note menu, and close. */
const Buttons = ({ roll }: { roll: Roll }) => {
  const { song, edits } = roll.page;
  const large = song.memory.editorLarge;
  const keys = (action: string) => {
    const key = song.shortcut(action);
    return key === null ? undefined : keysOf(key);
  };
  return (
    <row h={HEADER} px={2} gap={1} align="center">
      <Button id="editor-size" label={large ? "Make the note editor smaller" : "Make the note editor larger"} icon={large ? "chevron-down" : "chevron-up"} variant="ghost" size="sm"
        tip={large ? "Smaller" : "Larger"} keys={keys("editor_size")} onPress={() => (song.memory.editorLarge = !large)} />
      <Menu id="note-actions" label="Quantize, humanize, legato" variant="ghost" size="sm" tip="Quantize, humanize, legato" w={220} items={noteItems(roll)} />
      <Button id="close-editor" label="Close the note editor" icon="x" variant="ghost" size="sm" tip="Close" keys={keys("back")} onPress={() => edits.tracks.openPanel(roll.track.name)} />
    </row>
  );
};

/** The lanes under the notes: the velocity and the four of every clip, the clip's own controllers, and every other one where the track's instrument hears them. */
function laneOptions(roll: Roll): { value: NoteLane; text: string; detail?: string }[] {
  const lane = roll.page.song.memory.lane;
  const own = Object.keys(roll.state.controllers).map(Number).sort((a, b) => a - b);
  const hears = roll.track.children.some((child) => child.name === "synth" && String(child.tool) === "plugin");
  const others = hears ? CONTROLLERS : typeof lane === "number" ? [lane] : [];
  const numbers = [...own, ...others.filter((number) => !own.includes(number))];
  return [
    ...LANES.map((each) => ({ value: each, text: LANE_NAMES[each] })),
    ...numbers.map((number) => {
      const { label, detail } = controllerName(number);
      return { value: number, text: label, ...(detail ? { detail } : {}) };
    }),
  ];
}
