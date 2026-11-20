import { h, Live, live, memo, type Color } from "../../../sdk";
import { noteKey, pitchName } from "../../../song/notes";
import { barsOf, LANE, MOVE, playheadOf, signatureOf, START, ticksOf, VELOCITY, type Roll } from "./roll-view";

/** How many dots draw the line a drag across the lane draws. */
const DOTS = 24;

/**
 * The velocity of each note as a bar from the bottom, the chosen ones bright and on top: a drag
 * of a bar changes every chosen note as much; a drag across the lane draws a line, and each note
 * it passes gets the velocity of its height there. Down the lane is `127 - velocity`.
 */
export const VelocityLane = ({ roll }: { roll: Roll }) => {
  const key = `${roll.key}-velocity`;
  const line = live.drag(key);
  const { at, inside } = playheadOf(roll);
  return (
    <box key={key} label="Velocity lane" grow={1} h={LANE - 1} clip plane={{ x: roll.key, y: [-10, 128] }} drag={{ x: true, y: true }} onDragEnd={(p) => roll.edits.drawVelocities(p)}>
      {barsOf(roll.state.notes).map(([bar, notes]) =>
        memo(`velocities-${roll.key}-${bar}`, [signatureOf(roll, notes), roll.across, roll.colour], () => {
          const chosen = (note: (typeof notes)[number]) => roll.chosen.has(noteKey(note.note));
          return <box>{[...notes.filter((note) => !chosen(note)), ...notes.filter(chosen)].map(({ note, index }) => <Bar roll={roll} note={note} index={index} />)}</box>;
        }),
      )}
      {Array.from({ length: DOTS }, (_, dot) => (
        <box place={[line.x.plus(line.dx.times(dot / (DOTS - 1))), line.y.plus(line.dy.times(dot / (DOTS - 1)))]} w={3} h={3} radius="full" fill="text" opacity={line.active} inert />
      ))}
      <box place={[at, 59]} w={1} h={1000} fill="text" opacity={inside} inert />
    </box>
  );
};

const Bar = ({ roll, note, index }: { roll: Roll; note: Roll["state"]["notes"][number]; index: number }) => {
  const chosen = roll.chosen.has(noteKey(note));
  const key = `velocity-${index}`;
  const [move, front, loud] = chosen ? [live.drag(MOVE), live.drag(START), live.drag(VELOCITY)] : [null, null, live.drag(key)];
  const x = move && front ? move.dx.plus(front.dx).plus(note.start).max(0) : Live.of(note.start);
  const top = loud.dy.plus(127 - note.velocity).clamp(0, 126);
  const colour = chosen ? "text" : roll.colour;
  return (
    <box key={key} label={`Velocity of note ${pitchName(note.pitch)} ${index + 1}`} role="button" value={note.velocity} place={[x, top, ticksOf(roll, 3), Live.of(127).minus(top)]} fill={`${colour}/${chosen ? 100 : 70}` as Color}
      drag={{ y: true, id: chosen ? VELOCITY : key }} onPress={(p) => roll.edits.press(note, p)} onDragEnd={(p) => (!chosen && roll.edits.choose(note), roll.edits.velocity(p))}>
      <box at={[-2, -3]} w={7} h={7} radius="full" fill={colour} />
    </box>
  );
};
