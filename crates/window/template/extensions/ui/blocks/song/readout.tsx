import { h, Live, live, type Child } from "../../../sdk";
import { lengthText, noteKey, noteValueOf, pitchName } from "../../../song/notes";
import { BEAT, beatRuns, placeOf, tickOfPlace } from "../../../song/time";
import { END, HEADER, MOVE, SHORTEST, START, VELOCITY, type Roll } from "./roll-view";

/** A whole note, which a length is said in parts of. */
const WHOLE = 4 * BEAT;

/**
 * What the chosen note is, the earliest of them: its pitch, where it starts in the song as bar,
 * beat and sixteenth, how long it is in notes of the snap, and its velocity. Each follows a drag
 * before it lands. A press on one, or Enter, types it: with several chosen, their pitches and
 * starts move as far as this one's, and their lengths and velocities become what is typed.
 */
export const Readout = ({ roll }: { roll: Roll }) => {
  const chosen = roll.state.notes.filter((note) => roll.chosen.has(noteKey(note)));
  const note = chosen.reduce<(typeof chosen)[number] | undefined>((first, each) => (!first || each.start < first.start ? each : first), undefined);
  if (!note) return null;
  const { edits, page } = roll;
  const runs = page.song.clock.tempo_map.time_signatures;
  const snap = page.song.memory.snap;
  const [move, front, back, loud] = [live.drag(MOVE), live.drag(START), live.drag(END), live.drag(VELOCITY)];
  const at = move.dx.plus(front.dx).plus(note.start).round().max(0).plus(roll.state.start);
  const value = noteValueOf(note.length, snap);
  // How far into its beat the start is, by the beats of each time signature.
  const into = beatRuns(runs).reduce((sum, run, index, all) => {
    const next = all[index + 1];
    const inside = next ? at.ge(run.from).times(at.lt(next.from)) : at.ge(run.from);
    return sum.plus(inside.times(at.minus(run.from).mod(run.ticks)));
  }, Live.of(0));
  return (
    <row h={HEADER} px={2} gap={1} align="center" border="border" borderSides={[0, 0, 0, 1]}>
      {chosen.length > 1 && <text style="label" color="text-muted">{`${chosen.length} notes`}</text>}
      <Typed id="pitch" word="Pitch" text={pitchName(note.pitch)} onCommit={(text) => edits.typePitch(text)}>
        <text style="number" color="text" live={Live.of(note.pitch).minus(move.dy).clamp(0, 127)} unit="pitch" />
      </Typed>
      <Typed id="start" word="Start" text={placeOf(runs, roll.state.start + note.start).join(".")} onCommit={(text) => edits.typeStart(tickOfPlace(runs, text))}>
        <text style="number" color="text" live={at} unit="position" />
        <text style="number" color="text">.</text>
        <text style="number" color="text" live={into.over(BEAT / 4).floor().plus(1)} digits={0} />
      </Typed>
      <Typed id="length" word="Length" text={lengthText(note.length, snap)} onCommit={(text) => edits.typeLength(text)}>
        <text style="number" color="text" live={back.dx.minus(front.dx).plus(note.length).max(SHORTEST).over(WHOLE / value).round()} digits={0} />
        <text style="number" color="text">{`/${value}`}</text>
      </Typed>
      <Typed id="velocity" word="Velocity" text={String(note.velocity)} onCommit={(text) => edits.typeVelocity(text)}>
        <text style="number" color="text" live={loud.dy.negate().plus(note.velocity).clamp(1, 127).round()} digits={0} />
      </Typed>
    </row>
  );
};

/** A value with the word that names it: a press, or Enter, types it in its place; text that says what it said changes nothing. */
const Typed = ({ id, word, text, onCommit, children }: { id: string; word: string; text: string; onCommit: (text: string) => void; children?: Child }) => (
  <row key={`note-${id}`} label={`Type the ${word.toLowerCase()}`} role="button" value={text} h={24} px={2} gap={2} align="center" radius="sm" hover="fill">
    <text style="label" color="text-muted">{word}</text>
    <row align="center">{children}</row>
    <row overlay={{ open: "press", side: "bottom", align: "start", gap: -24 }} h={24} px={1} align="center" radius="md" fill="raised" border="focus" shadow="md">
      <field key={`note-${id}-typed`} label={`${word} of the note`} value={text} style="number" w={72} focus onCommit={(typed) => typed !== null && typed.trim() !== text && onCommit(typed.trim())} />
    </row>
  </row>
);
