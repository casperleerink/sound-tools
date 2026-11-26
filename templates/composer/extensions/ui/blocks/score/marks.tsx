import { h, live, memo, type Num, type Pointer } from "../../../sdk";
import { changeBars } from "../../../song/marks";
import { BAR, barAt, barStarts, BEAT, tickOfBar } from "../../../song/time";
import { GUTTER, HEAD, PLANE, type Page } from "../../../song/view";
import { beatOf, signatureOf } from "../../../score/bars";
import { bpmOf, chosenBars, HOLD, HOLDS, playingAt, RIT, tempoMarks, type TempoMark, type Typing } from "../../../score/edits";
import { MenuList } from "../../menu";
import { TextField } from "../../text-field";
import { BarMenu, barsOf, type Score } from "./bar-menu";

/** The bars track over the music, its lines from the top: the sections, the tempo marks, the time signatures and the bar numbers. */
export const [BAND, SECTIONS, TEMPOS, SIGNATURES, NUMBERS] = [100, 6, 32, 58, 62];
/** The least points between two bar numbers. */
const APART = 28;

/** Points from the left edge of what starts at `from` while it goes on past the edge, so its label stays in view. */
export const pinned = (from: number, inset = 4): Num => {
  const view = live.plane(PLANE);
  return view.left.minus(from).max(0).times(view.zoom).plus(inset);
};

/** The letter of the section `index` from 0, as rehearsal marks count: A to Z, then A2. */
export const letterOf = (index: number) => String.fromCharCode(65 + (index % 26)) + (index >= 26 ? String(Math.floor(index / 26) + 1) : "");

/** The bars track over the music, in its time: a mark is changed where it stands, and a press or a drag elsewhere chooses bars. */
export const Marks = ({ page, score }: { page: Page; score: Score }) => {
  const { song } = page;
  const { clock, memory } = song;
  const end = score.end;
  const runs = clock.tempo_map.time_signatures;
  const chosen = chosenBars(song);
  const drag = live.drag("score-marks");
  const barOf = (tick: number) => barAt(runs, Math.min(Math.max(tick, 0), end - 1)).bar;
  return (
    <row w="full" h={BAND} border="border" borderSides={[0, 0, 1, 0]}>
      <Head page={page} score={score} />
      <box w={GUTTER} />
      <box key="score-marks" label="Bars" grow={1} h={BAND} plane={{ x: PLANE }} drag={{ x: true }}
        onPress={(p) => {
          const bar = barOf(p.x);
          if (p.right) return score.edits.chooseForMenu(bar);
          score.edits.choose(bar, p.shift);
          clock.jump(tickOfBar(runs, bar));
        }}
        onDragEnd={(p) => score.edits.chooseAcross(barOf(p.x), barOf(p.x + p.dx))}>
        {chosen && <box place={[tickOfBar(runs, chosen.first), 0, tickOfBar(runs, chosen.last + 1) - tickOfBar(runs, chosen.first), BAND]} fill="selected" inert />}
        <box place={[drag.x.min(drag.x.plus(drag.dx)), 0, drag.dx.abs(), BAND]} fill="selected" opacity={drag.active} inert />
        {memo("score-sections", [clock.sections, runs, end, memory.mark, memory.renamingSection], () => <Sections page={page} score={score} />)}
        {memo("score-tempos", [clock.tempo_map, end, memory.typing], () => <Tempos page={page} score={score} />)}
        {memo("score-numbers", [runs, end], () => <Numbers starts={barStarts(runs, end)} changes={changeBars(runs)} />)}
        {memo("score-signatures", [runs, end, memory.typing], () => <Signatures page={page} score={score} />)}
        <box place={[live.playhead, BAND - 5]} w={7} h={7} radius="full" fill="text" inert />
        <BarMenu page={page} score={score} id="marks-menu" />
      </box>
    </row>
  );
};

/** The head of the bars track: its name, and while bars are chosen, a button that opens their menu. */
const Head = ({ page, score }: { page: Page; score: Score }) => {
  const chosen = chosenBars(page.song);
  return (
    <col w={HEAD} h="full" pad={[3, 2]} gap={1} border="border" borderSides={[0, 1, 0, 0]}>
      <text style="label" color="text-secondary">Bars</text>
      {chosen ? (
        <row key="bars-button" label={`What to do with ${barsOf(chosen.first, chosen.last).toLowerCase()}`} role="button" h={24} pad={[2, 0]} gap={1} align="center" radius="md" fill="fill" hover="fill-strong">
          <text style="label" color="text">{barsOf(chosen.first, chosen.last)}</text>
          <icon name="chevron-down" size={14} color="text-secondary" />
          <BarMenu page={page} score={score} id="bars-button-menu" open="press" />
        </row>
      ) : (
        <text style="label" color="text-muted">Choose bars to change them</text>
      )}
    </col>
  );
};

/** The rehearsal letter and name of each section, kept at the left edge while the section goes on past it: a press chooses it and plays from it, a double press names it. */
const Sections = ({ page, score }: { page: Page; score: Score }) => {
  const end = score.end;
  const { song, edits } = page;
  const { clock, memory } = song;
  const starts = barStarts(clock.tempo_map.time_signatures, end);
  return (
    <box>
      {clock.sections.map((section, index) => {
        const from = starts[section.bar - 1] ?? end;
        const to = starts[(clock.sections[index + 1]?.bar ?? Infinity) - 1] ?? end;
        const chosen = memory.mark?.kind === "section" && memory.mark.bar === section.bar;
        const mark = { kind: "section" as const, bar: section.bar, tick: from };
        const letter = letterOf(index);
        if (memory.renamingSection?.bar === section.bar) {
          return (
            <box place={[from, SECTIONS, 0, 0]}>
              <box at={[4, -2]}><TextField id="section-name" label={`Name of section ${letter}`} value={section.name} focus size="sm" w={160} onCommit={(text) => edits.marks.nameSection(text)} /></box>
            </box>
          );
        }
        return (
          <box place={[from, SECTIONS, Math.max(to - from, 0), 22]} clip>
            <row key={`section-${section.bar}`} label={`Section ${letter} ${section.name}`} role="button" value={chosen} at={[pinned(from), 0]} h={22} gap={2} pad={[0, 0]} align="center" radius="sm" hover="fill"
              onPress={(p) => (p.right ? score.edits.chooseForMenu(section.bar) : edits.marks.pressMark(mark, p))}>
              <row w={22} h={22} align="center" justify="center" radius="sm" fill={chosen ? "selected" : undefined} border={chosen ? "text" : "text-muted"}>
                <text style="heading" color="text">{letter}</text>
              </row>
              <text style="label" color={chosen ? "text" : "text-secondary"}>{section.name}</text>
            </row>
          </box>
        );
      })}
    </box>
  );
};

/** A tempo as a score writes it: a quarter note, then what it equals. */
const Metronome = ({ bpm }: { bpm: number }) => (
  <row gap={1} align="center">
    <icon name="note" size={14} color="text-secondary" />
    <text style="number" color="text-secondary">= {String(bpmOf(bpm))}</text>
  </row>
);

/** Where a typed tempo goes, as wide as a tempo mark. */
const FIELD = 64;

/**
 * Each tempo mark where it differs from what played before, kept at the left edge while it holds;
 * each rit. or accel. as its word, a dashed line and where it goes; each fermata over the end of the
 * beat it holds. A press on a tempo or where a move goes types it there.
 */
const Tempos = ({ page, score }: { page: Page; score: Score }) => {
  const { song } = page;
  const { clock, memory } = song;
  const marks = tempoMarks(clock.tempo_map.tempo_changes);
  const typing = memory.typing;
  const typed = (target: Typing) => (text: string | null) => score.edits.typed(target, text);
  return (
    <box>
      {marks.map((mark, index) => {
        const to = marks[index + 1]?.tick ?? score.end;
        const bar = barAt(clock.tempo_map.time_signatures, mark.tick).bar;
        const field = typing?.kind === "tempo" && typing.tick === mark.tick;
        return (
          <box>
            {field ? (
              <box place={[mark.tick, TEMPOS - 2, 0, 0]}>
                <box at={[4, 0]}><TextField id="tempo-field" label={`Tempo at bar ${bar}`} value={String(bpmOf(mark.bpm))} focus size="sm" w={FIELD} onCommit={typed({ kind: "tempo", tick: mark.tick })} /></box>
              </box>
            ) : mark.shows && (
              <box place={[mark.tick, TEMPOS, Math.max(to - mark.tick, 0), 20]} clip>
                <row key={`tempo-${mark.tick}`} label={`Tempo ${bpmOf(mark.bpm)} at bar ${bar}`} role="button" at={[pinned(mark.tick), 0]} h={20} pad={[1, 0]} radius="sm" hover="fill"
                  onPress={(p) => (p.right ? score.edits.chooseForMenu(bar) : (memory.typing = { kind: "tempo", tick: mark.tick }))}>
                  <Metronome bpm={mark.bpm} />
                </row>
              </box>
            )}
            {mark.glide && <Glide page={page} mark={mark} glide={mark.glide} after={mark.shows || field ? FIELD + 8 : 4} score={score} />}
          </box>
        );
      })}
      {typing && typing.kind !== "signature" && !marks.some((mark) => mark.tick === typing.tick && (typing.kind === "tempo" || mark.glide)) && (
        <box place={[typing.tick, TEMPOS - 2, 0, 0]}>
          <row at={[4, 0]} gap={1} align="center" pad={[1, 0]} radius="sm" fill="background">
            {typing.kind === "tempo" ? (
              [<icon name="note" size={14} color="text-secondary" />,
                <TextField id="tempo-field" label={`Tempo at bar ${barAt(clock.tempo_map.time_signatures, typing.tick).bar}`} value={String(playingAt(clock.tempo_map, typing.tick))} focus size="sm" w={FIELD} onCommit={typed(typing)} />]
            ) : (
              [<text style="label" color="text-secondary">rit. or accel. to</text>,
                <TextField id="target-field" label={`Where the tempo goes from bar ${barAt(clock.tempo_map.time_signatures, typing.tick).bar}`} value={String(bpmOf(playingAt(clock.tempo_map, typing.tick) * RIT))} focus size="sm" w={FIELD} onCommit={typed(typing)} />]
            )}
          </row>
        </box>
      )}
      {(clock.tempo_map.fermatas ?? []).map((fermata) => <Fermata page={page} score={score} tick={fermata.tick} hold={fermata.hold} />)}
    </box>
  );
};

/** A rit. or an accel. from `mark`: its word `after` points in, a dashed line, and where it goes at its end; a press on the word or the tempo types where it goes. */
const Glide = ({ page, mark, glide, after, score }: { page: Page; mark: TempoMark; glide: NonNullable<TempoMark["glide"]>; after: number; score: Score }) => {
  const { memory } = page.song;
  const word = glide.to < mark.bpm ? "rit." : "accel.";
  const bar = barAt(page.song.clock.tempo_map.time_signatures, mark.tick).bar;
  const typing = memory.typing?.kind === "target" && memory.typing.tick === mark.tick;
  const type = (p: Pointer) => (p.right ? score.edits.chooseForMenu(bar) : (memory.typing = { kind: "target", tick: mark.tick }));
  // A dash every half beat, a quarter beat long, from after the word to before where it goes.
  const dashes: number[] = [];
  for (let tick = mark.tick + BEAT; tick < glide.end - BEAT; tick += BEAT / 2) dashes.push(tick, 10, tick + BEAT / 4, 10);
  return (
    <box>
      <shapes place={[0, TEMPOS, glide.end, 20]} units={[glide.end, 20]} lines={[{ color: "text-muted", width: 1, points: dashes }]} />
      <box place={[mark.tick, TEMPOS, 0, 0]}>
        <row key={`glide-${mark.tick}`} label={`${word} from bar ${bar}`} role="button" at={[after, 0]} h={20} pad={[1, 0]} align="center" radius="sm" fill="background" hover="fill" onPress={type}>
          <text style="label" color="text-secondary">{word}</text>
        </row>
      </box>
      <box place={[glide.end, TEMPOS - (typing ? 2 : 0), 0, 0]}>
        {typing ? (
          <box at={[-FIELD - 4, 0]}><TextField id="target-field" label={`Where the ${word} from bar ${bar} goes`} value={String(bpmOf(glide.to))} focus size="sm" w={FIELD} onCommit={(text) => score.edits.typed({ kind: "target", tick: mark.tick }, text)} /></box>
        ) : (
          <row key={`target-${mark.tick}`} label={`The ${word} from bar ${bar} goes to ${bpmOf(glide.to)}`} role="button" at={[-40, 0]} w={36} h={20} pad={[1, 0]} justify="end" align="center" radius="sm" fill="background" hover="fill" onPress={type}>
            <text style="number" color="text-secondary">{String(bpmOf(glide.to))}</text>
          </row>
        )}
      </box>
    </box>
  );
};

/** A fermata over the end of the beat it holds, on the top line, clear of where a rit. goes: a press opens how long it holds, and takes it out. */
const Fermata = ({ page, score, tick, hold }: { page: Page; score: Score; tick: number; hold: number }) => {
  const runs = page.song.clock.tempo_map.time_signatures;
  const bar = barAt(runs, tick).bar;
  const beat = beatOf(runs, bar);
  const id = `fermata-${tick}`;
  const items = [
    ...HOLDS.map((times) => ({ text: times === HOLD ? "Hold twice as long" : `Hold ${times} times as long`, checked: hold === times, onPress: () => score.edits.hold(tick, times) })),
    { text: "Remove the fermata", icon: "trash-2" as const, danger: true, line: true, onPress: () => score.edits.removeFermata(tick) },
  ];
  return (
    <box place={[tick + beat, SECTIONS, 0, 0]}>
      <row key={id} label={`Fermata at the end of bar ${bar}`} role="button" at={[-22, 1]} w={20} h={20} align="center" justify="center" radius="sm" hover="fill">
        <icon name="fermata" size={18} color="text-secondary" />
        <MenuList id={id} label={`Fermata at the end of bar ${bar}`} items={items} w={220} />
      </row>
    </box>
  );
};

/** The bar numbers at the foot, after the time signature where one starts: as the score zooms out, every second, fourth, eighth … shows, the first always. */
const Numbers = ({ starts, changes }: { starts: number[]; changes: number[] }) => {
  const { zoom } = live.plane(PLANE);
  return (
    <box>
      {starts.slice(0, -1).map((tick, index) => {
        const apart = index & -index || 64;
        const next = starts[index + apart] ?? starts.at(-1)! + apart * BAR;
        const shows = index === 0 ? 1 : zoom.ge(APART / Math.max(next - tick, 1));
        return (
          <box place={[tick, NUMBERS, 0, 0]} opacity={shows} inert>
            <text at={[changes.includes(index + 1) ? SIGNATURE + 10 : 4, 0]} style="number" color="text-muted">{index + 1}</text>
          </box>
        );
      })}
    </box>
  );
};

/** How wide a time signature's numbers are, in points. */
const SIGNATURE = 14;

/** The time signature where it changes, its two numbers stacked as a score writes them, kept at the left edge while it holds: a press types another for its bars. */
const Signatures = ({ page, score }: { page: Page; score: Score }) => {
  const { song } = page;
  const { memory } = song;
  const runs = song.clock.tempo_map.time_signatures;
  const starts = barStarts(runs, score.end);
  const firsts = changeBars(runs);
  const typing = memory.typing?.kind === "signature" ? memory.typing : null;
  const field = (typed: NonNullable<typeof typing>) => (
    <box place={[starts[typed.bar - 1] ?? 0, NUMBERS - 4, 0, 0]}>
      <box at={[4, 0]}>
        <TextField id="signature-field" label={`Time signature at bar ${typed.bar}`} value={signatureOf(runs, typed.bar)} focus size="sm" w={64} onCommit={(text) => score.edits.typed(typed, text)} />
      </box>
    </box>
  );
  return (
    <box>
      {firsts.map((bar, index) => {
        const from = starts[bar - 1];
        if (from === undefined || from >= score.end) return null;
        const signature = runs[index]!.signature;
        if (typing?.bar === bar) return field(typing);
        const to = Math.min(starts[(firsts[index + 1] ?? Infinity) - 1] ?? score.end, score.end);
        const last = barAt(runs, to - 1).bar;
        const [count, unit] = signature.split("/");
        return (
          <box place={[from, SIGNATURES, to - from, 34]} clip>
            <col key={`signature-${bar}`} label={`Time signature ${signature} at bar ${bar}`} role="button" at={[pinned(from, 0), 0]} w={SIGNATURE + 6} align="center" radius="sm" fill="background" hover="fill"
              onPress={(p) => (p.right ? score.edits.chooseForMenu(bar) : (memory.typing = { kind: "signature", bar, last }))}>
              <text style="heading" color="text-secondary">{count}</text>
              <text style="heading" color="text-secondary">{unit}</text>
            </col>
          </box>
        );
      })}
      {typing && !firsts.includes(typing.bar) && field(typing)}
    </box>
  );
};
