import { h, live, memo, type Color } from "../../../sdk";
import { noteKey, pitchName } from "../../../song/notes";
import { barsOf, LANE, MOVE, playing, signatureOf, START, VELOCITY, type Roll } from "./roll-view";
import { Playhead } from "../time";

/** How many dots draw the line a drag across the lane draws. */
const DOTS = 24;
/** Down the lane, from above the loudest to under the softest, and its points per velocity. */
const RANGE: [number, number] = [-10, 128];
const PER = (LANE - 1) / (RANGE[1] - RANGE[0]);
/** How tall a bar is, in points: past the lane's foot from any velocity. */
const TALL = 2 * LANE;

/**
 * The velocity of each note as a bar from the bottom, the chosen ones bright and on top: a drag
 * of a bar changes every chosen note as much; a drag across the lane draws a line, and each note
 * it passes gets the velocity of its height there. Down the lane is `127 - velocity`.
 */
export const VelocityLane = ({ roll }: { roll: Roll }) => {
  const key = `${roll.key}-velocity`;
  const line = live.drag(key);
  return (
    <box key={key} label="Velocity lane" grow={1} h={LANE - 1} clip plane={{ x: roll.key, y: RANGE }} drag={{ x: true, y: true }} onDragEnd={(p) => roll.edits.drawVelocities(p)}>
      {barsOf(roll.state.notes).map(([bar, notes]) =>
        memo(`velocities-${roll.key}-${bar}`, [signatureOf(roll, notes), roll.across, roll.colour], () => {
          const chosen = (note: (typeof notes)[number]) => roll.chosen.has(noteKey(note.note));
          return <box>{[...notes.filter((note) => !chosen(note)), ...notes.filter(chosen)].map(({ note, index }) => <Bar roll={roll} note={note} index={index} />)}</box>;
        }),
      )}
      {Array.from({ length: DOTS }, (_, dot) => (
        <box place={[line.x.plus(line.dx.times(dot / (DOTS - 1))), line.y.plus(line.dy.times(dot / (DOTS - 1)))]} w={3} h={3} radius="full" fill="text" opacity={line.active} inert />
      ))}
      <Playhead plane={key} {...playing(roll)} />
    </box>
  );
};

/**
 * A bar from the top of its velocity down past the lane's foot, which cuts it: only its top
 * follows a drag. A mark of the plane, so its width is in points at any zoom. One not chosen
 * follows no drag, as a zoom walks every element: it lands where it is let go.
 */
const Bar = ({ roll, note, index }: { roll: Roll; note: Roll["state"]["notes"][number]; index: number }) => {
  const chosen = roll.chosen.has(noteKey(note));
  const key = `velocity-${index}`;
  const top = 127 - note.velocity + TALL / 2 / PER;
  const colour = chosen ? "text" : roll.colour;
  return (
    <box key={key} label={`Velocity of note ${pitchName(note.pitch)} ${index + 1}`} role="button" value={note.velocity}
      place={chosen ? [live.drag(MOVE).dx.plus(live.drag(START).dx).plus(note.start).max(0), live.drag(VELOCITY).dy.clamp(note.velocity - 127, note.velocity - 1).plus(top)] : [note.start, top]} w={4} h={TALL}
      radius="sm" fill={`${colour}/${chosen ? 100 : 70}` as Color} drag={{ y: true, id: chosen ? VELOCITY : key }} onPress={(p) => roll.edits.press(note, p)} onDragEnd={(p) => (!chosen && roll.edits.choose(note), roll.edits.velocity(p))}>
    </box>
  );
};
