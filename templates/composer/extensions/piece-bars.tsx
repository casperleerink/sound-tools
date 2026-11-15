import { Arc, Button, Circle, For, Fragment, h, Label, Line, memo, now, Rect, Show, Space, Text, untrack, type Color, type Drag, type Live, type Num, type Press, type SpaceValues, type TextPart } from "./sdk";
import { addBar, askSignature, askSlide, askTempo, choiceMenu, choiceOf, chooseRows, clearChosen, fermataMenu, moveFermata } from "./piece-choice";
import { moveSection } from "./piece-edits";
import { beatOf, countOf, endOf, formText, slidesOn, tempoOf, type Bar } from "./piece-form";
import { ACROSS, AFTER, barAtTick, chooseBars, clamp, DIM, HEADER, LEAD, LOW_ROW, MARKS, muted, PAPER, ROW, ZOOM, type Page } from "./piece-page";
import { addSectionAtEnd, chooseSection, nameSection, sectionMenu } from "./piece-sections";
import { addTrack } from "./piece-tracks";

// The whole piece on one line, engraved as a score lying on its side: over the system, as an
// engraver sets them, the rehearsal marks, the tempo words and the bar numbers; in it, barlines
// through every instrument, the time signatures large on the music, a double barline where a
// section ends and a final one at the end. Across, a unit is a tick; the rows scroll and zoom, the
// band follows them.

/** Where the lines of the band start: the rehearsal marks, the tempo words, the bar numbers. */
const [NAME_Y, TEMPO_Y, NUMBER_Y] = [8, 33, 56];
/** The bar-number line, where chosen whole bars show. */
const TRACK_Y = NUMBER_Y - 2;
/** The points a bar's number needs before the next one, and before a fermata on its bar; and which bars keep theirs as the room gets less, as a score numbers every fifth or tenth. */
const NUMBER_ROOM = 40;
const NUMBERED = [1000, 500, 100, 50, 10, 5, 1];
/** About how many points a character takes: of a label, of the tempo words, plain and bold, of a rehearsal mark, of a digit of a time signature. */
const [LABEL_CHAR, WORD_CHAR, BOLD_CHAR, MARK_CHAR, DIGIT] = [5.6, 5.4, 8, 7.6, 15];
/** A fermata over the system: where the middle of its arc is, and its radius. */
const [FERMATA_Y, FERMATA] = [MARKS - 4, 8];
const HINT = "Double-click a bar to write notes";
/** Where the piece opens: whole when a bar of 4/4 keeps this many points, else at zoom 1. */
const NARROWEST = 40;
const [FIT_BEFORE, FIT_AFTER] = [12, 120];
/** How wide the buttons on chosen whole bars are together, and the points they keep from an edge or the last row. */
const [BUTTONS, GAP] = [152, 8];
/** The dashes after `rit.` or `accel.`: this many points long, as far apart, and at most this many. */
const [DASH, DASHES] = [4, 120];
/** The radius of a note's head, in points. */
const HEAD = 2.6;

/** 1 while `ticks` give at least `points` of room at the zoom it shows: the window works it out as it zooms. */
const fits = ({ space }: Page, ticks: number, points: number) => space.pointX.le(ticks / points);
const not = (value: Live) => value.negate().plus(1);
/** A live 0, for a mark with nothing to say. */
const NONE = now.tick.times(0);

/** A tempo as the band engraves it: the note of its beat, drawn, `dotted` or not, or none, then its words, the number bold. */
type Words = { note: boolean; dotted: boolean; parts: TextPart[]; points: number };
const wordsOf = (bar: Bar | null, ...parts: TextPart[]): Words => ({
  note: bar !== null, dotted: bar !== null && beatOf(bar.signature).quarters !== 1, parts, points: parts.reduce((sum, part) => sum + part.text.length * (part.weight === "semibold" ? BOLD_CHAR : WORD_CHAR), 0),
});
const bold = (text: string): TextPart => ({ text, weight: "semibold" });
const plain = (text: string): TextPart => ({ text });
/** The points a drawn note takes before its words, a dotted one more; and those words take. */
const widthOf = ({ note, dotted, points }: Words) => (note ? (dotted ? 17 : 13) : 0) + points;

/**
 * A tempo mark at each bar where the tempo changes: the forms it shows, from the longest, each
 * with the points it needs. Bars that slow down or speed up along one straight line have one mark,
 * `rit.` and dashes up to where they end, and the bar after them says its tempo again.
 */
function temposOf({ all, end }: Page) {
  const { bars, starts } = all();
  const slides = (index: number) => bars[index]?.to !== undefined;
  const rises = (index: number) => endOf(bars[index]!) > bars[index]!.bpm;
  const said = (bar: Bar, bpm = bar.bpm) => `${beatOf(bar.signature).sign} = ${tempoOf({ ...bar, bpm })}`;
  const tempoAt = (index: number) => index === 0 || (slides(index) ? !slidesOn(bars, index) : slides(index - 1) || said(bars[index - 1]!) !== said(bars[index]!));
  const changes = bars.flatMap((bar, index) => (tempoAt(index) ? [{ index, tick: starts[index]!, bar }] : []));
  return changes.map((change, at) => {
    const { bar, index } = change;
    const next = bars.findIndex((_, each) => each > index && tempoAt(each));
    const run = { from: index, to: next < 0 ? bars.length - 1 : next - 1 };
    const last = bars[run.to]!;
    const sliding = slides(index);
    const word = rises(index) ? "accel." : "rit.";
    const reached = tempoOf({ ...last, bpm: endOf(last) });
    // A slide that does not start where the bar before it ends says the tempo it starts at.
    const jumps = index === 0 || endOf(bars[index - 1]!) !== bar.bpm;
    const forms: Words[] = sliding
      ? [...(jumps ? [wordsOf(bar, bold(`= ${tempoOf(bar)}`), plain(`  ${word}`))] : []), wordsOf(null, plain(`${word}`))]
      : [wordsOf(bar, bold(`= ${tempoOf(bar)}`)), wordsOf(null, bold(String(tempoOf(bar))))];
    return {
      ...change, run, word, reached, sliding,
      forms: forms.map((words) => ({ words, width: widthOf(words) + 14 })),
      room: (changes[at + 1]?.tick ?? end()) - change.tick,
      slideEnd: sliding ? starts[run.to + 1]! : null,
    };
  });
}

/** Each bar where the time signature changes, the digits it shows, the bars it stands for and the tick of the next. */
export function signaturesOf({ all, end }: Page) {
  const { bars, starts } = all();
  const at = (index: number) => index === 0 || bars[index - 1]!.signature !== bars[index]!.signature;
  const changes = bars.flatMap((bar, index) => (at(index) ? [index] : []));
  return changes.map((index, each) => {
    const [beats, note] = bars[index]!.signature.split("/") as [string, string];
    const next = changes[each + 1];
    return { index, tick: starts[index]!, beats, note, width: Math.max(beats.length, note.length) * DIGIT, run: { from: index, to: next === undefined ? bars.length - 1 : next - 1 }, next: next === undefined ? end() : starts[next]! };
  });
}

/** The sections, each from the tick it starts at to where it ends. */
export const sectionsOf = ({ sections, form }: Page) =>
  sections().flatMap((section, index, all) => {
    const place = form().places[section.id];
    return place ? [{ id: section.id, name: section.name.trim() || "Section", from: place.start, to: place.start + place.length, last: index === all.length - 1 }] : [];
  });

/** The chosen bars, from where the first starts to where the last ends, in ticks, and the rows of the chosen instruments: all 0 while nothing is chosen. */
export const chosenSpan = (page: Page) => {
  const { starts, rows } = page;
  const choice = choiceOf(page);
  if (!choice) return { x: 0, width: 0, whole: 0, music: 0, all: 0, last: 0, top: 0, height: 0 };
  const [from, to] = [starts()[choice.from]!, starts()[choice.to + 1]!];
  const last = choice.to === choice.all.bars.length - 1 ? 1 : 0;
  const span = { x: from, width: to - from, all: choice.from === 0 && last ? 1 : 0, last };
  if (choice.rows === null) return { ...span, whole: 1, music: 0, top: 0, height: 0 };
  const indexes = choice.rows.map((name) => rows().findIndex((each) => each.name === name));
  const top = Math.min(...indexes);
  return { ...span, whole: 0, music: 1, top, height: Math.max(...indexes) - top + 1 };
};
/** The bar at the tick `x`, the last past the end of the piece. */
const barNear = ({ barAt, end }: Page, x: number) => barAt(clamp(x, 0, end() - 1))!;

/** A button that keeps its size in points at any zoom, at `x` and `y` in the units of `space`: words on the paper, `tone` the blue of a choice or quiet ink. */
const PaperButton = ({ space, id, x, y, text, width, shown, onClick, tone = "soft" }: {
  space: SpaceValues; id: string; x: () => Live; y: () => Num; text: string; width: number; shown: () => number; onClick: () => void; tone?: "soft" | "chosen";
}) => {
  const height = space.pointY.times(22);
  const chosen = tone === "chosen";
  return (
    <>
      <Rect x={x} y={y} width={space.pointX.times(width)} height={height} radius={4} color={PAPER.page} border={chosen ? PAPER.chosen : undefined} borderWidth={chosen ? 1 : 0} opacity={shown} />
      <Rect key={id} x={x} y={y} width={() => space.pointX.times(width).times(shown())} height={height} radius={4} color="none" hover={PAPER.hover} cursor="pointer" opacity={shown} onClick={onClick} />
      <Label x={() => x().plus(space.pointX.times(chosen ? width / 2 : 6))} y={() => space.pointY.times(3).plus(y())} align={chosen ? "center" : "left"} text={text} color={chosen ? PAPER.chosen : PAPER.soft} opacity={shown} />
    </>
  );
};

/** A fermata as a score prints it: an arc over a dot. */
const fermataSign = (x: Num | (() => Num), y: number, opacity: Num | (() => Num)) => [
  <Arc x={x} y={y} radius={FERMATA} from={-0.25} to={0.25} color={PAPER.ink} width={2} opacity={opacity} />,
  <Circle x={x} y={y - 2.5} radius={1.8} color={PAPER.ink} opacity={opacity} />,
];

/**
 * The band over the system. A press on a bar chooses it whole, all instruments, shift-press or a
 * drag a range, and puts the playhead there; a right press opens the menu of the chosen bars. A
 * click on a tempo or on `rit.` types it for the bars it stands for, a right press chooses its bar
 * as one over it does. A click on a fermata opens how long it holds, a drag moves it to another
 * count. A rehearsal mark chooses its section's bars, a double click names it, a right press opens
 * its menu, a drag moves it.
 */
export const Marks = ({ page }: { page: Page }) => {
  const { memory, all, starts, end, barAt, space, sections } = page;
  const band = now.space("marks");
  const tempos = memo(() => temposOf(page));
  const parts = memo(() => sectionsOf(page));
  const chosen = () => chosenSpan(page);
  const inChosen = (index: number) => {
    const at = memory.chosen;
    return at !== null && index >= at.from && index <= at.to;
  };
  /** 1 while the number of bar `index` shows: zoomed out, every fifth, tenth … bar keeps its room. */
  const numberShows = (index: number) => {
    const tick = starts()[index] ?? 0;
    const apart = NUMBERED.find((every) => (index + 1) % every === 0)!;
    // The first and the last chosen bar always say their number.
    const chosen = memory.chosen !== null && (index === memory.chosen.from || index === memory.chosen.to);
    const room = index === 0 || chosen ? not(NONE) : fits(page, (starts()[index + apart] ?? tick + apart * 4 * 960) - tick, NUMBER_ROOM);
    // Not under the fermata of its bar.
    const fermata = all().bars[index]?.fermata;
    return fermata ? room.times(fits(page, (fermata.beat - 0.5) * countOf(all().bars[index]!.signature), 30)) : room;
  };
  const fermatas = memo(() => all().bars.flatMap((bar, index) => (bar.fermata ? [{ index, middle: starts()[index]! + (bar.fermata.beat - 0.5) * countOf(bar.signature), end: starts()[index + 1]! }] : [])));
  /** Where a fermata is drawn: over the middle of its count, clear of the line its bar ends at. */
  const fermataX = (fermata: { middle: number; end: number }) => space.pointX.times(-(FERMATA + 5)).plus(fermata.end).min(fermata.middle);
  // A fermata carried along the band lands on the count it is let go on.
  let held: number | null = null;
  const hold = now.drag("fermata");
  const holdDrag: Drag = {
    id: "fermata",
    axis: "x",
    onEnd: ({ x }) => {
      const from = held;
      held = null;
      if (from !== null) moveFermata(page, from, clamp(x, 0, end() - 1));
    },
  };
  const pressFermata = (index: number) => ({ button }: Press) => {
    held = index;
    if (button === "right") void fermataMenu(page, index);
  };
  const sweep = now.drag("bars");
  const sweepDrag: Drag = {
    id: "bars",
    axis: "x",
    onEnd: ({ startX, x }) => {
      const from = barAt(startX);
      if (from !== null) chooseBars(page, from, barNear(page, x));
    },
  };
  const press = ({ x, shift, button }: Press) => {
    const bar = barAt(x);
    if (bar === null) {
      if (button === "left") memory.chosen = null;
      return;
    }
    const at = memory.chosen;
    const among = at !== null && at.rows === null && bar >= at.from && bar <= at.to;
    if (button === "right") {
      if (!among) chooseBars(page, bar, bar);
      void choiceMenu(page, x);
    } else if (shift && at !== null) chooseBars(page, at.anchor, bar);
    else chooseBars(page, bar, bar);
  };
  // A rehearsal mark carried along the band lands before the section it is let go on, after the last past the end.
  let carried: string | null = null;
  const carry = now.drag("section");
  const carryDrag: Drag = {
    id: "section",
    axis: "x",
    onEnd: ({ x }) => {
      const id = carried;
      carried = null;
      if (id === null) return;
      const before = x >= end() ? null : (parts().find((part) => x < part.to)?.id ?? null);
      if (before === id || before === (parts()[parts().findIndex((part) => part.id === id) + 1]?.id ?? null)) return;
      moveSection(page, id, before);
      // What was chosen would now be over other music.
      chooseSection(page, id);
    },
  };
  const landing = () =>
    parts().reduce((x, part, index) => x.plus(carry.x.ge(part.from).times(part.from - (parts()[index - 1]?.from ?? 0))), carry.x.times(0)).plus(carry.x.ge(end()).times(end() - (parts().at(-1)?.from ?? 0)));
  /** 1 while a drop would move nothing: on the carried section, its name being where the drag started, or on the one after it. */
  const landsInPlace = () =>
    parts().reduce((sum, part, index) => {
      const carried = carry.startX.ge(part.from).times(carry.startX.lt(part.to));
      const next = parts()[index + 1];
      const over = (index === 0 ? not(NONE) : carry.x.ge(part.from)).times(next ? carry.x.lt(next.to) : 1);
      return sum.plus(carried.times(over));
    }, carry.x.times(0));
  const pressName = (id: string) => ({ button }: Press) => {
    carried = id;
    if (button === "right") void sectionMenu(page, id);
  };
  const clickName = (id: string) => ({ clicks }: Press) => (clicks === 2 ? void nameSection(page, id) : chooseSection(page, id));
  // Not after an undo, which nothing here hears: the sections are then no longer as that edit left them.
  const notice = () => (memory.notice?.form === formText(sections() as Parameters<typeof formText>[0]) ? memory.notice.text : null);
  const playhead = now.tick.plus(space.pointX.times(0.5));
  const after = (points: number) => () => space.pointX.times(points).plus(end());

  return (
    <Space key="marks" height={MARKS} unit={{ x: ACROSS }} extent={() => ({ x: [-LEAD, end() + AFTER] })} zoom={{ x: ZOOM }} follow={{ x: "rows" }}
      background={PAPER.page} onPress={press} drag={sweepDrag} cursor="pointer">
      <Rect x={() => chosen().x} y={TRACK_Y} width={() => chosen().width} height={MARKS - TRACK_Y} color={PAPER.chosenFill} opacity={() => chosen().whole} />
      <Rect x={sweep.startX.min(sweep.x)} y={TRACK_Y} width={sweep.dx.abs()} height={MARKS - TRACK_Y} color={PAPER.chosenFill} opacity={sweep.active} />
      <For each={parts}>
        {(part) => {
          // The name boxed, or its first letter as a rehearsal mark, or a dot: never nothing.
          // The last one ends where "+ Section" starts.
          const room = () => (parts().find((each) => each.from > part.from)?.from ?? end()) - part.from;
          const widths = [part.name.length * MARK_CHAR + 22, 30];
          // Not while its name is typed: the field is where it is.
          const named = () => (memory.typing === `section-${part.id}` ? 0 : 1);
          const shows = [() => fits(page, room(), widths[0]!).times(named()), () => fits(page, room(), widths[1]!).times(not(fits(page, room(), widths[0]!))).times(named()), () => not(fits(page, room(), widths[1]!))];
          // Its name stays at the left edge while the section goes on past it, up to its end.
          const nameX = () => band.left.max(part.from).min(space.pointX.times(-widths[0]!).plus(part.to)).max(part.from);
          // Blue while all its bars, and only they, are chosen.
          const colour = (): Color => {
            const [at, bars] = [memory.chosen, all().bars];
            return at?.rows === null && bars[at.from]?.id === part.id && bars[at.to]?.id === part.id && bars[at.from]?.at === 0 && bars[at.to + 1]?.id !== part.id ? PAPER.chosen : PAPER.ink;
          };
          const box = (x: () => Num, width: number, text: string, shows: () => Live) => [
            <Rect x={() => space.pointX.times(1).plus(x())} y={NAME_Y - 3} width={space.pointX.times(width - 8)} height={24} radius={2} color={PAPER.page} border={colour} borderWidth={1.5} opacity={shows} />,
            <Label x={() => space.pointX.times(1 + (width - 8) / 2).plus(x())} y={NAME_Y} align="center" variant="body" opacity={shows} text={() => [{ text, color: colour(), weight: "semibold" }]} />,
          ];
          return [
            ...box(nameX, widths[0]!, part.name, shows[0]!),
            ...box(() => part.from, widths[1]!, part.name.slice(0, 1), shows[1]!),
            <Rect x={() => space.pointX.times(1).plus(part.from)} y={NAME_Y + 6} width={space.pointX.times(8)} height={8} radius={1} color={PAPER.ink} opacity={shows[2]} />,
            <Rect key={`section-${part.id}`} x={nameX} y={NAME_Y - 2} height={24} color="none" cursor="grab"
              width={() => space.pointX.times(widths[0]!).times(shows[0]()).plus(space.pointX.times(widths[1]!).times(shows[1]())).plus(space.pointX.times(12).times(shows[2]()))}
              drag={carryDrag} onPress={pressName(part.id)} onClick={clickName(part.id)} />,
          ];
        }}
      </For>
      <Rect x={() => landing().minus(space.pointX)} y={0} width={space.pointX.times(2)} height={MARKS} color="lavender" opacity={() => carry.active.times(carry.dx.abs().gt(0)).times(not(landsInPlace()))} />
      <For each={tempos}>
        {(mark) => {
          // The longest form that has room before the next mark, else a dot.
          const shown = mark.forms.map((form, index) => {
            const room = fits(page, mark.room, form.width);
            const before = mark.forms[index - 1];
            return before ? room.times(not(fits(page, mark.room, before.width))) : room;
          });
          const dot = not(fits(page, mark.room, mark.forms.at(-1)!.width));
          const shownWidth = mark.forms.reduce((sum, form, index) => sum.plus(shown[index]!.times(form.width - 14)), NONE);
          // Its words stay at the left edge while the bars it stands for go on past it, up to the next mark.
          const tempoX = () => band.left.plus(band.pointX.times(6)).min(space.pointX.times(shownWidth.negate().minus(10)).plus(mark.tick + mark.room)).max(space.pointX.times(5).plus(mark.tick));
          // A click types for the bars the mark stands for, which it chooses, so they show; a right press chooses the bar of the mark, as one on the band over it does.
          const typeTempo = (event: Press) => {
            if (event.button !== "left") return press({ ...event, x: mark.tick });
            chooseBars(page, mark.run.from, mark.run.to);
            memory.typing = `tempo-${mark.index}`;
            void (mark.sliding ? askSlide : askTempo)(page, mark.run).finally(() => (memory.typing = null));
          };
          // While a field is over a mark, that mark hides under it and the others are faint, so none shows cut in half beside it.
          const unless = (opacity: Num) => () => {
            const [kind, index] = memory.typing?.split("-") ?? [];
            if (kind !== "tempo") return opacity;
            return Number(index) === mark.index ? 0 : typeof opacity === "number" ? opacity * 0.3 : opacity.times(0.3);
          };
          // The dashes of a slide, one a beat, from its words to where it ends.
          const dashes = (() => {
            if (mark.slideEnd === null) return [];
            const slideEnd = mark.slideEnd;
            const wordsEnd = () => space.pointX.times(shownWidth.plus(6)).plus(tempoX());
            const y = TEMPO_Y + 11;
            return [
              ...Array.from({ length: DASHES }, (_, at) => {
                const from = () => space.pointX.times(at * DASH * 2).plus(wordsEnd());
                return (
                  <Line from={() => [from(), y] as const} to={() => [space.pointX.times(DASH).plus(from()), y] as const} color={PAPER.soft} width={1.2}
                    opacity={unless(space.pointX.times(DASH + 4).plus(from()).lt(slideEnd).times(not(dot)))} />
                );
              }),
              <Rect key={`slide-${mark.index}`} x={wordsEnd} y={TEMPO_Y - 4} width={() => wordsEnd().negate().plus(slideEnd).max(0)} height={24} color="none" cursor="pointer" onPress={typeTempo} />,
            ];
          })();
          // A tempo: the note of its beat drawn, a head and a stem and its dot, so it sits right by its words, then the words.
          const tempoWords = ({ note, dotted, parts }: Words, shows: Num) => {
            const [across, opacity] = [(points: number) => () => space.pointX.times(points).plus(tempoX()), unless(shows)];
            return [
              ...(note
                ? [
                    <Circle x={across(3.5)} y={TEMPO_Y + 13} radius={3.2} color={PAPER.ink} opacity={opacity} />,
                    <Line from={() => [across(6.3)(), TEMPO_Y + 13] as const} to={() => [across(6.3)(), TEMPO_Y + 1] as const} color={PAPER.ink} width={1.3} opacity={opacity} />,
                    ...(dotted ? [<Circle x={across(10.5)} y={TEMPO_Y + 13} radius={1.3} color={PAPER.ink} opacity={opacity} />] : []),
                  ]
                : []),
              <Label x={across(note ? (dotted ? 16 : 12) : 0)} y={TEMPO_Y} variant="body" text={parts} color={PAPER.ink} opacity={opacity} />,
            ];
          };
          return [
            ...dashes,
            ...mark.forms.flatMap((form, index) => [
              tempoWords(form.words, shown[index]!),
              <Rect key={`tempo-${mark.index}-${index}`} x={() => space.pointX.times(-4).plus(tempoX())} y={TEMPO_Y - 4} width={space.pointX.times(form.width - 6).times(shown[index]!)} height={24} color="none" cursor="pointer" onPress={typeTempo} />,
            ]),
            <Circle x={() => space.pointX.times(4).plus(mark.tick)} y={TEMPO_Y + 9} radius={2} color={PAPER.soft} opacity={unless(dot)} />,
            <Rect key={`tempo-${mark.index}-dot`} x={mark.tick} y={TEMPO_Y - 4} width={space.pointX.times(12).times(dot)} height={24} color="none" cursor="pointer" onPress={typeTempo} />,
          ];
        }}
      </For>
      <For each={() => starts().slice(0, -1)}>
        {(tick, index) => (
          <Label x={() => space.pointX.times(2).plus(tick)} y={NUMBER_Y} text={() => String(index() + 1)} color={() => (inChosen(index()) ? PAPER.chosen : PAPER.faint)} opacity={() => numberShows(index())} />
        )}
      </For>
      <For each={fermatas}>
        {(fermata) => [
          ...fermataSign(() => fermataX(fermata), FERMATA_Y, 1),
          <Rect key={`fermata-${fermata.index}`} x={() => space.pointX.times(-10).plus(fermataX(fermata))} y={NUMBER_Y} width={space.pointX.times(20)} height={MARKS - NUMBER_Y} color="none" cursor="pointer"
            drag={holdDrag} onPress={pressFermata(fermata.index)}
            onClick={() => {
              chooseBars(page, fermata.index, fermata.index);
              void fermataMenu(page, fermata.index);
            }} />,
        ]}
      </For>
      {fermataSign(hold.x, FERMATA_Y, () => hold.active.times(hold.dx.abs().gt(0)).times(0.5))}
      <PaperButton space={band} id="add-section" x={after(16)} y={() => NAME_Y - 2} text="+ Section" width={76} shown={() => 1} onClick={() => void addSectionAtEnd(page, true)} />
      <PaperButton space={band} id="add-bar-end" x={after(16)} y={() => TEMPO_Y - 3} text="+ Bar" width={76} shown={() => 1 - chosen().whole * chosen().last} onClick={() => addBar(page, "end")} />
      <Line from={() => [playhead.minus(space.pointX.times(4)), MARKS - 6] as const} to={() => [playhead, MARKS] as const} color={PAPER.playhead} width={1.5} />
      <Line from={() => [playhead.plus(space.pointX.times(4)), MARKS - 6] as const} to={() => [playhead, MARKS] as const} color={PAPER.playhead} width={1.5} />
      <Show when={() => notice() !== null}>
        {() => [
          <Rect x={band.left} y={NAME_Y - 4} width={() => band.pointX.times((notice()?.length ?? 0) * LABEL_CHAR + 24)} height={22} color={PAPER.page} />,
          <Label x={() => band.left.plus(band.pointX.times(12))} y={NAME_Y} text={() => notice() ?? ""} color="#a3470f" />,
        ]}
      </Show>
    </Space>
  );
};
/** Whether no instrument plays a note in any section: the first row then says how to write some. */
const silentOf = ({ rows }: Page) => memo(() => rows().every((each) => each.children.every((child) => child.tool !== "timeline" || child.state.notes.length === 0)));

/** Where the clock repeats, in ticks: none while it does not. */
const repeatSpan = ({ clock }: Page) => {
  const repeat = clock.repeat;
  return repeat ? { from: repeat.from, to: repeat.to, shown: 1 } : { from: 0, to: 0, shown: 0 };
};

/**
 * The system: a row per instrument with the notes of its parts in ink, barlines through all of
 * them, a double barline where a section ends, a final one at the end, and the time signatures
 * large on the music; repeat barlines around what loops. A press chooses that bar of that
 * instrument and the instrument, shift-press takes in the bars and instruments up to it, a drag
 * those it goes over; a right press opens the menu of what is chosen; a double click opens the
 * notes of its part. A click on a time signature types it for the bars it stands for. A press under
 * the rows lets go. Under chosen whole bars, the buttons that add a bar after them and remove them.
 */
export const Rows = ({ page }: { page: Page }) => {
  const { rows, returnsHeight, starts, end, all, memory, clock, partAt, openPanel, openEditor, barAt, space, size } = page;
  const silent = silentOf(page);
  const chosen = () => chosenSpan(page);
  const names = () => rows().map((each) => each.name);
  const parts = memo(() => sectionsOf(page));
  const signatures = memo(() => signaturesOf(page));
  const height = () => rows().length;
  /** The barlines inside sections; a section's ends are double, the piece's end final. */
  const lines = memo(() => starts().filter((tick) => tick < end() && !parts().some((part) => part.to === tick)));
  const press = ({ x, y, clicks, shift, button }: Press) => {
    const each = rows()[Math.floor(y)];
    const bar = barAt(x);
    if (!each) {
      if (button === "left") memory.chosen = null;
      return;
    }
    if (bar === null) {
      memory.chosen = null;
      return openPanel(each.name);
    }
    const at = choiceOf(page);
    if (button === "right") {
      const among = at && (at.rows === null || at.rows.includes(each.name)) && bar >= at.from && bar <= at.to;
      if (!among) chooseRows(page, bar, bar, [each.name]);
      void choiceMenu(page, x);
    } else if (clicks === 2) openEditor(each.name);
    else if (shift && at?.rows) {
      const indexes = [names().indexOf(each.name), names().indexOf(at.rows[0]!), names().indexOf(at.rows.at(-1)!)];
      chooseRows(page, at.anchor, bar, names().slice(Math.min(...indexes), Math.max(...indexes) + 1));
    } else if (shift && at) chooseBars(page, at.anchor, bar);
    else chooseRows(page, bar, bar, [each.name]);
  };
  // A drag chooses whole bars of whole rows: the bars and rows under its ends.
  const sweep = now.drag("music");
  const sweepDrag: Drag = {
    id: "music",
    onEnd: ({ startX, startY, x, y }) => {
      const last = rows().length - 1;
      const [from, to] = [barAt(startX), barNear(page, x)];
      const [top, bottom] = [clamp(Math.floor(Math.min(startY, y)), 0, last), clamp(Math.floor(Math.max(startY, y)), 0, last)];
      if (from !== null && last >= 0) chooseRows(page, from, to, names().slice(top, bottom + 1));
    },
  };
  // The whole piece when a bar of 4/4 keeps its room, else a bar of 4/4 at 96 points.
  const opening = untrack(() => {
    const room = Math.max(size.width - HEADER - FIT_BEFORE - FIT_AFTER, 1);
    return end() > 0 && (room / end()) * 4 * 960 >= NARROWEST ? end() : room / ACROSS;
  });
  // The rows scroll the least to show what is chosen as it changes, and the bar the playhead is in as the piece starts to play while the page follows it.
  type Shown = { x: readonly [number, number]; y?: readonly [number, number] } | undefined;
  let [shownChoice, wasPlaying, shown]: [string | undefined, boolean, Shown] = [undefined, false, undefined];
  const toShow = memo((): Shown => {
    const choice = choiceOf(page);
    const at = chosen();
    const range: Shown = choice && { x: [starts()[choice.from]!, starts()[choice.to + 1]!], ...(at.music ? { y: [at.top, at.top + at.height] } : {}) };
    const playing = clock.playing && memory.follows;
    if (JSON.stringify(range) !== shownChoice) shown = range;
    else if (playing && !wasPlaying) {
      // A space scrolls only for a range other than the last: one tick, which no choice is, and none once it stops, so each start scrolls.
      const start = starts()[barAtTick(all(), untrack(() => clock.tick))] ?? 0;
      shown = { x: [start, start + 1] };
    } else if (!playing && wasPlaying) shown = undefined;
    [shownChoice, wasPlaying] = [JSON.stringify(range), playing];
    return shown;
  });
  const playhead = now.tick.plus(space.pointX.times(0.5));
  /** A barline `points` wide, its right edge `before` points left of `tick`. */
  const barline = (tick: number, points = 1, before = 0) => (
    <Rect x={() => space.pointX.times(-(before + points)).plus(tick)} y={0} width={space.pointX.times(points)} height={height} color={PAPER.ink} />
  );
  const repeat = () => repeatSpan(page);
  // The buttons on chosen whole bars: at their left edge, a little under the last row or over the bottom of what shows, never past an edge.
  const buttonsX = () => space.left.plus(space.pointX.times(GAP)).max(chosen().x).min(space.right.minus(space.pointX.times(BUTTONS + GAP)));
  const buttonsY = () => space.bottom.minus(space.pointY.times(22 + GAP)).min(rows().length + GAP / ROW);
  return (
    <>
      <div style={{ grow: true }}>
        <Space key="rows" grow unit={{ x: ACROSS, y: ROW }} extent={() => ({ x: [-LEAD, end() + AFTER], y: [0, rows().length + 1] })} zoom={{ x: ZOOM }} background={PAPER.page}
          fit={{ x: [0, opening], before: FIT_BEFORE, after: FIT_AFTER }} show={toShow} turns={() => (memory.follows ? now.tick : undefined)} onPress={press} drag={sweepDrag}>
          <Rect x={() => chosen().x} y={0} width={() => chosen().width} height={height} color={PAPER.chosenFill} border={PAPER.chosen} borderWidth={1.5} opacity={() => chosen().whole} />
          <Rect x={() => chosen().x} y={() => chosen().top + 3 / ROW} width={() => chosen().width} height={() => chosen().height - 6 / ROW} radius={3}
            color={PAPER.chosenFill} border={PAPER.chosen} borderWidth={1.5} opacity={() => chosen().music} />
          <Rect x={sweep.startX.min(sweep.x)} y={sweep.startY.min(sweep.y)} width={sweep.dx.abs()} height={sweep.dy.abs()} radius={3}
            color={PAPER.chosenFill} border={PAPER.chosen} borderWidth={1} opacity={sweep.active} />
          <For each={lines}>{(tick) => <Rect x={tick} y={0} width={space.pointX} height={height} color={PAPER.bar} />}</For>
          <For each={parts}>
            {(part) => (part.last ? [barline(part.to, 1, 5), barline(part.to, 3.5)] : [barline(part.to, 1, 3), barline(part.to, 1)])}
          </For>
          <For each={rows}>
            {(each, index) => {
              /** Its notes in every section, each where it plays in the piece. */
              const notes = memo(() =>
                all().bars.length === 0 ? [] : parts().flatMap((part) => (partAt(each, part.id)?.state.notes ?? []).map((note) => ({ ...note, start: note.start + part.from }))),
              );
              // Between its lowest and highest pitch, a line each, an octave tall at least, in the middle of the row.
              const miniature = memo(() => {
                const pitches = notes().map((note) => note.pitch);
                const [lowest, highest] = [Math.min(...pitches), Math.max(...pitches)];
                const range = highest - lowest + 1;
                const shown = Math.max(range, 12);
                const step = (ROW - 26) / shown;
                const height = clamp(step, 2, 3.5);
                const bottom = ROW - 13 - ((shown - range) / 2) * step;
                return { lowest, step, height, bottom };
              });
              return (
                <For each={notes}>
                  {(note) => {
                    // As proportional notation draws a note: its head where it starts, a line as long as it lasts.
                    const y = () => {
                      const { bottom, lowest, step } = miniature();
                      return index() + (bottom - (note.pitch - lowest + 0.5) * step) / ROW;
                    };
                    const opacity = () => (muted(each) ? DIM : 1);
                    return [
                      <Rect x={note.start} y={() => y() - 0.6 / ROW} width={space.pointX.times(-1).plus(note.length).max(space.pointX)} height={1.2 / ROW} color={PAPER.soft} opacity={opacity} />,
                      <Circle x={() => space.pointX.times(HEAD).plus(note.start)} y={y} radius={HEAD} color={PAPER.ink} opacity={opacity} />,
                    ];
                  }}
                </For>
              );
            }}
          </For>
          <For each={signatures}>
            {(mark) => {
              const typing = () => memory.typing === `signature-${mark.index}`;
              const colour = (): Color => (typing() ? PAPER.chosen : PAPER.soft);
              const x = () => space.pointX.times(6).plus(mark.tick);
              const fitsHere = fits(page, mark.next - mark.tick, mark.width + 12);
              return [
                <For each={rows}>
                  {(_, index) => {
                    const middle = () => index() + 0.5;
                    return [
                      <Rect x={() => space.pointX.times(-3).plus(x())} y={() => middle() - 27 / ROW} width={space.pointX.times(mark.width + 6)} height={54 / ROW} color={PAPER.veil} opacity={fitsHere} />,
                      <Label x={() => space.pointX.times(mark.width / 2).plus(x())} y={() => middle() - 29 / ROW} align="center" variant="display" text={[{ text: mark.beats, weight: "semibold" }]} color={colour} opacity={fitsHere} />,
                      <Label x={() => space.pointX.times(mark.width / 2).plus(x())} y={() => middle() - 5 / ROW} align="center" variant="display" text={[{ text: mark.note, weight: "semibold" }]} color={colour} opacity={fitsHere} />,
                    ];
                  }}
                </For>,
                <Rect key={`signature-${mark.index}`} x={() => space.pointX.times(-3).plus(x())} y={0} width={space.pointX.times(mark.width + 6).times(fitsHere)} height={height} color="none" cursor="pointer"
                  onPress={(event) => {
                    if (event.button !== "left") return press(event);
                    chooseBars(page, mark.run.from, mark.run.to);
                    memory.typing = `signature-${mark.index}`;
                    void askSignature(page, mark.run).finally(() => (memory.typing = null));
                  }} />,
              ];
            }}
          </For>
          {(() => {
            // Repeat barlines, as a score has them, around what loops.
            const [from, to, shows] = [() => repeat().from, () => repeat().to, () => repeat().shown];
            const at = (tick: () => number, points: number) => () => space.pointX.times(points).plus(tick());
            const dots = (x: () => Live) => (
              <For each={rows}>
                {(_, index) => [
                  <Circle x={x} y={() => index() + 0.5 - 7 / ROW} radius={2.4} color={PAPER.ink} opacity={shows} />,
                  <Circle x={x} y={() => index() + 0.5 + 7 / ROW} radius={2.4} color={PAPER.ink} opacity={shows} />,
                ]}
              </For>
            );
            return [
              <Rect x={from} y={0} width={space.pointX.times(3.5)} height={height} color={PAPER.ink} opacity={shows} />,
              <Rect x={at(from, 6)} y={0} width={space.pointX} height={height} color={PAPER.ink} opacity={shows} />,
              dots(at(from, 11)),
              <Rect x={at(to, -3.5)} y={0} width={space.pointX.times(3.5)} height={height} color={PAPER.ink} opacity={shows} />,
              <Rect x={at(to, -7)} y={0} width={space.pointX} height={height} color={PAPER.ink} opacity={shows} />,
              dots(at(to, -11)),
            ];
          })()}
          <Show when={() => silent() && rows().length > 0 && memory.chosen === null}>
            {() => [
              <Rect x={() => space.left.max(0).plus(space.pointX.times(6))} y={0.5 - 12 / ROW} width={space.pointX.times(HINT.length * LABEL_CHAR + 12)} height={24 / ROW} radius={4} color={PAPER.page} />,
              <Label x={() => space.left.max(0).plus(space.pointX.times(12))} y={0.5 - 8 / ROW} text={HINT} color={PAPER.faint} />,
            ]}
          </Show>
          <Line from={() => [playhead, 0] as const} to={() => [playhead, rows().length] as const} color={PAPER.playhead} width={1.5} />
          <PaperButton space={space} id="chosen-add-bar" tone="chosen" x={buttonsX} y={buttonsY} text="+ Bar" width={44} shown={() => chosen().whole} onClick={() => addBar(page, "after")} />
          <PaperButton space={space} id="chosen-remove" tone="chosen" x={() => buttonsX().plus(space.pointX.times(48))} y={buttonsY} text="Remove" width={56} shown={() => chosen().whole * (1 - chosen().all)}
            onClick={() => clearChosen(page, true)} />
          <PaperButton space={space} id="chosen-more" tone="chosen" x={() => buttonsX().plus(space.pointX.times(108 * chosen().whole))} y={buttonsY} text="More" width={44} shown={() => chosen().whole + chosen().music}
            onClick={() => void choiceMenu(page)} />
        </Space>
        <Show when={() => rows().length === 0}>{() => <EmptyPiece page={page} />}</Show>
      </div>
      <div style={() => ({ height: returnsHeight() + LOW_ROW, keep: true, background: "background" })} />
    </>
  );
};

/** Over the rows while the piece has no instrument: what to start with, as the window's empty project says it. */
const EmptyPiece = ({ page }: { page: Page }) => (
  <div style={{ absolute: { top: 0, left: 0, right: 0, bottom: 0 }, align: "center", justify: "center", gap: "xs" }}>
    <Text variant="title" style={{ color: PAPER.ink }}>An empty piece</Text>
    <Text style={{ color: PAPER.soft }}>Add an instrument to write for, or ask the agent for one.</Text>
    <div style={{ paddingTop: "md" }}>
      <Button key="empty-instrument" label="Instrument" icon="plus" variant="subtle" size="md" onClick={() => addTrack(page, 0)} />
    </div>
  </div>
);
