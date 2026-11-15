import { Arc, Button, Circle, For, Fragment, h, Label, Line, memo, now, Path, Rect, Show, Space, Text, untrack, type Drag, type Live, type Num, type Press, type SpaceValues } from "./sdk";
import { addBar, askSignature, askSlide, askTempo, choiceMenu, choiceOf, chooseRows, clearChosen, fermataMenu, moveFermata } from "./piece-choice";
import { moveSection } from "./piece-edits";
import { beatOf, countOf, endOf, formText, partsOf, slidesOn, tempoOf, type Bar } from "./piece-form";
import { ACROSS, AFTER, barAtTick, chooseBars, clamp, colourOf, DIM, HEADER, LEAD, LOW_ROW, MARKS, muted, ROW, washOf, ZOOM, type Page } from "./piece-page";
import { addSectionAtEnd, chooseSection, nameSection, sectionMenu } from "./piece-sections";
import { addTrack } from "./piece-tracks";

// The whole piece on one line, drawn so its shape reads from afar: each section a wash of its own
// down the whole page, with its rehearsal letter; the tempo one line over the piece, which jumps,
// slopes for a rit. or accel. and breaks for a fermata; the bars a ruler whose ticks are the
// counts, with the time signature where it changes. Under the band a row of notes per instrument.
// Across, a unit is a tick; the rows scroll and zoom, the band follows them.

/** The three lanes of the band, from the top: the sections, the tempo, the ruler of bars. */
const [TEMPO_TOP, RULER_TOP] = [40, MARKS - 40];
/** Where a section's letter and name start. */
const [LETTER_Y, NAME_Y] = [8, 13];
/** The heights of the tempo line: the fastest tempo of the piece, the slowest; its words sit over it. */
const [FAST, SLOW, WORDS_ABOVE] = [TEMPO_TOP + 22, RULER_TOP - 8, 19];
/** The two digits of a time signature, a bar's number, and how tall a bar's tick and a count's are. */
const [TOP_DIGIT, BOTTOM_DIGIT, NUMBER_Y, BAR_TICK, COUNT_TICK] = [RULER_TOP + 3, RULER_TOP + 19, RULER_TOP + 6, 12, 4];
/** The points a bar's number needs before the next one; and which bars keep theirs as the room gets less, as a score numbers every fifth or tenth. */
const NUMBER_ROOM = 90;
const NUMBERED = [1000, 500, 100, 50, 10, 5, 1];
/** About how many points a character takes: of a label, of a title, of a digit of a time signature, of a rehearsal letter. */
const [LABEL_CHAR, TITLE_CHAR, DIGIT, LETTER] = [5.6, 8.6, 10, 34];
const FERMATA = 7;
const HINT = "Double-click a bar to write notes";
/** Where the piece opens: whole when a bar of 4/4 keeps this many points, else at zoom 1. */
const NARROWEST = 40;
const [FIT_BEFORE, FIT_AFTER] = [12, 120];
/** How wide the two buttons on chosen whole bars are together, and the points they keep from an edge or the last row. */
const [BUTTONS, GAP] = [152, 6];

/** 1 while `ticks` give at least `points` of room at the zoom it shows: the window works it out as it zooms. */
const fits = ({ space }: Page, ticks: number, points: number) => space.pointX.le(ticks / points);
const not = (value: Live) => value.negate().plus(1);
/** A live 0, for a mark with no time signature. */
const NONE = now.tick.times(0);

const said = (bar: Bar, bpm = bar.bpm) => `${beatOf(bar.signature).sign} = ${tempoOf({ ...bar, bpm })}`;
/** A tempo as the band writes it: the note of its beat, drawn, `dotted` or not, or none, then its words. */
type Words = { note: boolean; dotted: boolean; text: string };
const plain = (text: string): Words => ({ note: false, dotted: false, text });
const withNote = (bar: Bar, text: string): Words => ({ note: true, dotted: beatOf(bar.signature).quarters !== 1, text });
const widthOf = ({ note, dotted, text }: Words) => (note ? (dotted ? 15 : 11) : 0) + text.length * LABEL_CHAR;

/** The height of the tempo line at `bpm`, in quarters a minute: higher is faster, by ratio, so a doubling is as tall anywhere. */
function heightsOf({ all }: Page) {
  const speeds = all().bars.flatMap((bar) => [bar.bpm, endOf(bar)]);
  const [low, high] = [Math.min(...speeds), Math.max(...speeds)];
  return (bpm: number) => (high <= low ? (FAST + SLOW) / 2 : SLOW - (Math.log(bpm / low) / Math.log(high / low)) * (SLOW - FAST));
}

/** The tempo line, as runs of points: a step where the tempo jumps, a slope where it slides, and a break over the count a fermata holds, with where each fermata is. */
function tempoLineOf(page: Page) {
  const { bars, starts } = page.all();
  const heightOf = heightsOf(page);
  const runs: [number, number][][] = [[]];
  const fermatas: { index: number; from: number; to: number; y: number }[] = [];
  bars.forEach((bar, index) => {
    const [from, to] = [starts[index]!, starts[index + 1]!];
    const at = (tick: number): [number, number] => [tick, heightOf(bar.bpm + ((endOf(bar) - bar.bpm) * (tick - from)) / (to - from))];
    if (!bar.fermata) return runs.at(-1)!.push(at(from), at(to));
    const count = countOf(bar.signature);
    const [held, after] = [from + (bar.fermata.beat - 1) * count, from + bar.fermata.beat * count];
    runs.at(-1)!.push(at(from), at(held));
    runs.push([at(after), at(to)]);
    fermatas.push({ index, from: held, to: after, y: Math.min(at(held)[1], at(after)[1]) });
  });
  return { runs: runs.filter((run) => run.length > 1), fermatas };
}

/**
 * The tempo marks: one where the tempo is said again, each with the bars it stands for, the forms
 * of its words from the longest, and how high they sit. Bars that slow down or speed up along one
 * straight line are one mark, `rit. to 60`; the bar after them says its tempo again.
 */
function tempoMarksOf(page: Page) {
  const { bars, starts } = page.all();
  const heightOf = heightsOf(page);
  const slides = (index: number) => bars[index]?.to !== undefined;
  const tempoAt = (index: number) => index === 0 || (slides(index) ? !slidesOn(bars, index) : slides(index - 1) || said(bars[index - 1]!) !== said(bars[index]!));
  const at = bars.flatMap((_, index) => (tempoAt(index) ? [index] : []));
  return at.map((index, place) => {
    const bar = bars[index]!;
    const to = (at[place + 1] ?? bars.length) - 1;
    const last = bars[to]!;
    const sliding = slides(index);
    const word = endOf(bar) > bar.bpm ? "accel." : "rit.";
    const reached = tempoOf({ ...last, bpm: endOf(last) });
    // A slide that does not start where the bar before it ends says the tempo it starts at.
    const jumps = index === 0 || endOf(bars[index - 1]!) !== bar.bpm;
    const forms = sliding
      ? [...(jumps ? [withNote(bar, `= ${tempoOf(bar)}  ${word} ${reached}`)] : []), plain(`${word} ${reached}`), plain(word)]
      : [withNote(bar, `= ${tempoOf(bar)}`), plain(String(tempoOf(bar)))];
    const [top, bottom] = [Math.min(heightOf(bar.bpm), heightOf(endOf(last))), Math.max(heightOf(bar.bpm), heightOf(endOf(last)))];
    return { index, tick: starts[index]!, next: starts[to + 1]!, run: { from: index, to }, sliding, forms, y: top - WORDS_ABOVE, bottom };
  });
}

/** Where the time signature changes, with the bars it stands for. */
function signatureMarksOf({ all }: Page) {
  const { bars, starts } = all();
  const at = bars.flatMap((bar, index) => (index === 0 || bars[index - 1]!.signature !== bar.signature ? [index] : []));
  return at.map((index, place) => {
    const to = (at[place + 1] ?? bars.length) - 1;
    const [beats, note] = bars[index]!.signature.split("/") as [string, string];
    return { index, tick: starts[index]!, next: starts[to + 1]!, run: { from: index, to }, beats, note, width: Math.max(beats.length, note.length) * DIGIT };
  });
}

/** The rehearsal letter of the section at `index`: A to Z, then AA. */
const letterOf = (index: number): string => (index < 26 ? String.fromCharCode(65 + index) : letterOf(Math.floor(index / 26) - 1) + letterOf(index % 26));

/** The sections, each from the tick it starts at to where it ends, with its place among them. */
const sectionsOf = ({ sections, form }: Page) =>
  sections().flatMap((section) => {
    const place = form().places[section.id];
    return place ? [{ id: section.id, name: section.name.trim() || "Section", from: place.start, to: place.start + place.length }] : [];
  }).map((part, index) => ({ ...part, index, letter: letterOf(index) }));

/** The chosen bars, from where the first starts to where the last ends, in ticks, and the rows of the chosen instruments: all 0 while nothing is chosen. */
const chosenSpan = (page: Page) => {
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

/** Where the clock repeats, in ticks: none while it does not. */
const repeatSpan = ({ clock }: Page) => {
  const repeat = clock.repeat;
  return repeat ? { x: repeat.from, width: repeat.to - repeat.from, shown: 1 } : { x: 0, width: 0, shown: 0 };
};

/** A button that keeps its size in points at any zoom, at `x` and `y` in the units of `space`: a quiet box with a label, over what it covers. */
const QuietButton = ({ space, id, x, y, text, width, shown, onClick, bare = false }: {
  space: SpaceValues; id: string; x: () => Live; y: () => Num; text: string; width: number; shown: () => number; onClick: () => void; bare?: boolean;
}) => {
  const height = space.pointY.times(20);
  return (
    <>
      <Rect x={x} y={y} width={space.pointX.times(width)} height={height} radius={4} color="background" opacity={shown} />
      <Rect key={id} x={x} y={y} width={() => space.pointX.times(width).times(shown())} height={height} radius={4} color={bare ? "none" : "fill"} hover={bare ? "fill" : "fill-strong"} cursor="pointer" opacity={shown} onClick={onClick} />
      <Label x={() => x().plus(space.pointX.times(bare ? 6 : width / 2))} y={() => space.pointY.times(2).plus(y())} align={bare ? "left" : "center"} text={text} color="text-secondary" opacity={shown} />
    </>
  );
};

/**
 * The band over the rows. A press on a bar chooses it whole, all instruments, shift-press or a
 * drag a range, and puts the playhead there; a right press opens the menu of the chosen bars. A
 * click on a time signature, or on the tempo line or its words, types it for the bars it stands
 * for, a right press chooses its bar as one over it does. A click on a fermata opens how long it
 * holds, a drag moves it to another count. A section's letter or name chooses its bars, a double
 * click names it, a right press opens its menu, a drag moves it.
 */
export const Marks = ({ page }: { page: Page }) => {
  const { memory, all, starts, end, barAt, space, sections } = page;
  const band = now.space("marks");
  const parts = memo(() => sectionsOf(page));
  const tempos = memo(() => tempoMarksOf(page));
  const signatures = memo(() => signatureMarksOf(page));
  const line = memo(() => tempoLineOf(page));
  const chosen = () => chosenSpan(page);
  const inChosen = (index: number) => {
    const at = memory.chosen;
    return at !== null && index >= at.from && index <= at.to;
  };
  /** 1 while the number of bar `index` shows: zoomed out, every second, fourth, eighth … bar keeps its room. */
  const numberShows = (index: number) => {
    const tick = starts()[index] ?? 0;
    const apart = NUMBERED.find((every) => (index + 1) % every === 0)!;
    // The first and the last chosen bar always say their number.
    const chosen = memory.chosen !== null && (index === memory.chosen.from || index === memory.chosen.to);
    return index === 0 || chosen ? not(NONE) : fits(page, (starts()[index + apart] ?? tick + apart * 4 * 960) - tick, NUMBER_ROOM);
  };
  /** Where bar `index`'s number starts: after its time signature, where one is. */
  const numberX = (index: number) => {
    const mark = signatures().find((each) => each.index === index);
    return mark ? mark.width + 10 : 4;
  };
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
  const fermataSign = (x: Num | (() => Num), y: number, opacity: Num | (() => Num)) => [
    <Arc x={x} y={y} radius={FERMATA} from={-0.25} to={0.25} color="text" width={1.5} opacity={opacity} />,
    <Circle x={x} y={y - 2} radius={1.5} color="text" opacity={opacity} />,
  ];
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
  // A section's name carried along the band lands before the section it is let go on, after the last past the end.
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
  const repeat = () => repeatSpan(page);
  const playhead = now.tick.plus(space.pointX.times(0.5));
  const after = (points: number) => () => space.pointX.times(points).plus(end());
  /** `x` points in from where a mark starts, or from the left edge while the mark goes on past it, up to `width` before where it ends. */
  const sticky = (from: number, to: number, width: number, x: number) => () => band.left.max(from).min(space.pointX.times(-width).plus(to)).max(from).plus(space.pointX.times(x));
  // A click types for the bars a mark stands for, which it chooses, so they show; a right press is a press on the band there.
  // A mark hides while its field, which says it, is over it, and the others of its kind are faint.
  const pressMark = (kind: "signature" | "tempo", index: number, run: { from: number; to: number }, type: () => Promise<unknown>) => (event: Press) => {
    if (event.button !== "left") return press(event);
    chooseBars(page, run.from, run.to);
    memory.typing = `${kind}-${index}`;
    void type().finally(() => (memory.typing = null));
  };
  const unless = (kind: "signature" | "tempo", index: number, opacity: Live) => () => {
    const [typed, at] = memory.typing?.split("-") ?? [];
    if (typed !== kind) return opacity;
    return Number(at) === index ? 0 : opacity.times(0.3);
  };

  return (
    <Space key="marks" height={MARKS} unit={{ x: ACROSS }} extent={() => ({ x: [-LEAD, end() + AFTER] })} zoom={{ x: ZOOM }} follow={{ x: "rows" }}
      onPress={press} drag={sweepDrag} cursor="pointer">
      <For each={parts}>
        {(part) => {
          // The letter and the name, or the letter alone; they stay at the left edge while the section goes on past it.
          const widths = [LETTER + 16 + part.name.length * TITLE_CHAR + 8, LETTER + 8];
          const room = part.to - part.from;
          // Not while its name is typed: the field is where it is.
          const named = () => (memory.typing === `section-${part.id}` ? 0 : 1);
          const shows = [() => fits(page, room, widths[0]!).times(named()), () => fits(page, room, widths[1]!).times(not(fits(page, room, widths[0]!))).times(named())];
          const x = sticky(part.from, part.to, widths[0]!, 0);
          const lettered = sticky(part.from, part.to, widths[1]!, 0);
          // Bright while all its bars, and only they, are chosen.
          const bright = () => {
            const [at, bars] = [memory.chosen, all().bars];
            return at?.rows === null && bars[at.from]?.id === part.id && bars[at.to]?.id === part.id && bars[at.from]?.at === 0 && bars[at.to + 1]?.id !== part.id;
          };
          const letter = (left: () => Live, opacity: () => Live) => (
            <Label x={() => left().plus(space.pointX.times(8))} y={LETTER_Y} variant="display" opacity={opacity}
              text={() => [{ text: ` ${part.letter} `, weight: "semibold", color: "text", border: bright() ? "text" : "text-muted" }]} />
          );
          return [
            <Rect x={part.from} y={0} width={room} height={MARKS} color={washOf(part.index)} />,
            ...(part.index > 0 ? [<Rect x={part.from} y={0} width={space.pointX} height={MARKS} color="border-strong" />] : []),
            letter(x, shows[0]!),
            <Label x={() => x().plus(space.pointX.times(LETTER + 16))} y={NAME_Y} variant="title" opacity={shows[0]} text={part.name} color={() => (bright() ? "text" : "text-secondary")} />,
            letter(lettered, shows[1]!),
            <Rect key={`section-${part.id}`} x={() => x().plus(space.pointX.times(4))} y={4} height={36} color="none" cursor="grab"
              width={() => space.pointX.times(widths[0]!).times(shows[0]()).plus(space.pointX.times(widths[1]!).times(shows[1]()))}
              drag={carryDrag} onPress={pressName(part.id)} onClick={clickName(part.id)} />,
          ];
        }}
      </For>
      <Rect x={() => chosen().x} y={RULER_TOP} width={() => chosen().width} height={MARKS - RULER_TOP - 1} radius={4} color="selected" border="text" borderWidth={1} opacity={() => chosen().whole} />
      <Rect x={sweep.startX.min(sweep.x)} y={RULER_TOP} width={sweep.dx.abs()} height={MARKS - RULER_TOP - 1} radius={4} color="fill" border="text-muted" borderWidth={1} opacity={sweep.active} />
      <Rect x={() => landing().minus(space.pointX)} y={0} width={space.pointX.times(2)} height={MARKS} color="lavender" opacity={() => carry.active.times(carry.dx.abs().gt(0)).times(not(landsInPlace()))} />
      <Show when={() => line().runs.length > 0}>
        {() => <Path points={() => { const all = line().runs.flat(); return [[all[0]![0], RULER_TOP], ...all, [all.at(-1)![0], RULER_TOP]]; }} color="text/7" />}
      </Show>
      <For each={() => line().runs}>{(run) => <Path points={() => run} color="text" width={2} />}</For>
      <For each={tempos}>
        {(mark) => {
          // The longest form that has room before the next tempo mark; none when none has.
          const room = mark.next - mark.tick;
          const shown = mark.forms.map((form, index) => {
            const fits_ = fits(page, room, widthOf(form) + 16);
            const before = mark.forms[index - 1];
            return before ? fits_.times(not(fits(page, room, widthOf(before) + 16))) : fits_;
          });
          const longest = widthOf(mark.forms[0]!) + 16;
          const x = sticky(mark.tick, mark.next, longest, 6);
          const typeTempo = pressMark("tempo", mark.index, mark.run, () => (mark.sliding ? askSlide : askTempo)(page, mark.run));
          const words = ({ note, dotted, text }: Words, shows: Live) => {
            const [across, opacity] = [(points: number) => () => space.pointX.times(points).plus(x()), unless("tempo", mark.index, shows)];
            const y = mark.y;
            return [
              ...(note
                ? [
                    <Circle x={across(3)} y={y + 11} radius={2.6} color="text" opacity={opacity} />,
                    <Line from={() => [across(5.2)(), y + 11] as const} to={() => [across(5.2)(), y + 2] as const} color="text" width={1.2} opacity={opacity} />,
                    ...(dotted ? [<Circle x={across(9.5)} y={y + 11} radius={1.1} color="text" opacity={opacity} />] : []),
                  ]
                : []),
              <Label x={across(note ? (dotted ? 15 : 11) : 0)} y={y} text={text} color="text" opacity={opacity} />,
            ];
          };
          return [
            ...mark.forms.flatMap((form, index) => words(form, shown[index]!)),
            // The words and the line under them, over all the bars the mark stands for.
            <Rect key={`tempo-${mark.index}`} x={mark.tick} y={mark.y - 3} width={room} height={mark.bottom - mark.y + 10} color="none" cursor="pointer" onPress={typeTempo} />,
          ];
        }}
      </For>
      <For each={() => line().fermatas}>
        {(fermata) => {
          const x = () => space.pointX.times(0).plus((fermata.from + fermata.to) / 2);
          const y = fermata.y - 12;
          return [
            ...fermataSign(x, y, 1),
            <Rect key={`fermata-${fermata.index}`} x={() => space.pointX.times(-10).plus(x())} y={y - FERMATA - 4} width={space.pointX.times(20)} height={FERMATA + 16} color="none" cursor="pointer"
              drag={holdDrag} onPress={pressFermata(fermata.index)}
              onClick={() => {
                chooseBars(page, fermata.index, fermata.index);
                void fermataMenu(page, fermata.index);
              }} />,
          ];
        }}
      </For>
      {fermataSign(hold.x, SLOW - 30, () => hold.active.times(hold.dx.abs().gt(0)).times(0.5))}
      <For each={() => starts().slice(0, -1)}>
        {(tick, index) => {
          const bar = () => all().bars[index()];
          const count = () => countOf(bar()?.signature ?? "4/4");
          const counts = () => Array.from({ length: partsOf(bar()?.signature ?? "4/4")[0] - 1 }, (_, at) => tick + (at + 1) * count());
          return [
            <Rect x={tick} y={MARKS - BAR_TICK} width={space.pointX} height={BAR_TICK} color="text-muted" />,
            <For each={counts}>{(at) => <Rect x={at} y={MARKS - COUNT_TICK} width={space.pointX} height={COUNT_TICK} color="text-muted" opacity={() => fits(page, count(), 3)} />}</For>,
            <Label x={() => space.pointX.times(numberX(index())).plus(tick)} y={NUMBER_Y} text={() => String(index() + 1)} color={() => (inChosen(index()) ? "text" : "text-muted")} opacity={() => numberShows(index())} />,
          ];
        }}
      </For>
      <For each={signatures}>
        {(mark) => {
          const room = mark.next - mark.tick;
          const shows = fits(page, room, mark.width + 12);
          const x = sticky(mark.tick, mark.next, mark.width + 12, 5 + mark.width / 2);
          const digit = (text: string, y: number) => (
            <Label x={x} y={y} align="center" variant="title" text={[{ text, weight: "semibold" }]} color="text" opacity={unless("signature", mark.index, shows)} />
          );
          return [
            digit(mark.beats, TOP_DIGIT), digit(mark.note, BOTTOM_DIGIT),
            <Rect key={`signature-${mark.index}`} x={() => x().minus(space.pointX.times(mark.width / 2 + 4))} y={RULER_TOP} width={() => space.pointX.times(mark.width + 8).times(shows)} height={MARKS - RULER_TOP - BAR_TICK + 4} color="none" cursor="pointer"
              onPress={pressMark("signature", mark.index, mark.run, () => askSignature(page, mark.run))} />,
          ];
        }}
      </For>
      <QuietButton space={band} id="add-bar-end" x={after(10)} y={() => TOP_DIGIT + 4} text="+ Bar" width={72} bare shown={() => 1 - chosen().whole * chosen().last} onClick={() => addBar(page, "end")} />
      <QuietButton space={band} id="add-section" x={after(10)} y={() => NAME_Y} text="+ Section" width={72} bare shown={() => 1} onClick={() => void addSectionAtEnd(page, true)} />
      <Rect x={() => repeat().x} y={MARKS - 3} width={() => repeat().width} height={2} radius={1} color="text" opacity={() => repeat().shown} />
      <Circle x={playhead} y={MARKS - 3} radius={2.5} color="text" />
      <Show when={() => notice() !== null}>
        {() => [
          <Rect x={band.left} y={NAME_Y - 4} width={() => band.pointX.times((notice()?.length ?? 0) * LABEL_CHAR + 24)} height={22} color="background" />,
          <Label x={() => band.left.plus(band.pointX.times(12))} y={NAME_Y} text={() => notice() ?? ""} color="peach" />,
        ]}
      </Show>
    </Space>
  );
};
/** Whether no instrument plays a note in any section: the first row then says how to write some. */
const silentOf = ({ rows }: Page) => memo(() => rows().every((each) => each.children.every((child) => child.tool !== "timeline" || child.state.notes.length === 0)));

/**
 * A row per instrument with the notes of its parts, small, in its colour, a thin line per bar and
 * a double line where a section ends, which the heads scroll with; under them the room beside the
 * returns and the master. A press chooses that bar of that instrument and the instrument,
 * shift-press takes in the bars and instruments up to it, a drag those it goes over; a right press
 * opens the menu of what is chosen; a double click opens the notes of its part. A press under the
 * rows lets go. Under chosen whole bars, the buttons that add a bar after them and remove them.
 */
export const Rows = ({ page }: { page: Page }) => {
  const { rows, returnsHeight, starts, end, all, memory, clock, partAt, openPanel, openEditor, barAt, space, size } = page;
  const silent = silentOf(page);
  const chosen = () => chosenSpan(page);
  const names = () => rows().map((each) => each.name);
  const parts = memo(() => sectionsOf(page));
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
  // The buttons on chosen whole bars: at their left edge, a little under the last row or over the bottom of what shows, never past an edge.
  const buttonsX = () => space.left.plus(space.pointX.times(GAP)).max(chosen().x).min(space.right.minus(space.pointX.times(BUTTONS + GAP)));
  const buttonsY = () => space.bottom.minus(space.pointY.times(20 + GAP)).min(rows().length + GAP / ROW);
  return (
    <>
      <div style={{ grow: true }}>
        <Space key="rows" grow unit={{ x: ACROSS, y: ROW }} extent={() => ({ x: [-LEAD, end() + AFTER], y: [0, rows().length + 1] })} zoom={{ x: ZOOM }}
          fit={{ x: [0, opening], before: FIT_BEFORE, after: FIT_AFTER }} show={toShow} turns={() => (memory.follows ? now.tick : undefined)} onPress={press} drag={sweepDrag}>
          <For each={parts}>
            {(part) => [
              <Rect x={part.from} y={space.top} width={part.to - part.from} height={() => space.bottom.minus(space.top)} color={washOf(part.index)} />,
              ...(part.index > 0 ? [<Rect x={part.from} y={space.top} width={space.pointX} height={() => space.bottom.minus(space.top)} color="border-strong" />] : []),
            ]}
          </For>
          <Rect x={() => chosen().x} y={0} width={() => chosen().width} height={() => rows().length} color="selected" opacity={() => chosen().whole} />
          <Rect x={() => chosen().x} y={() => chosen().top + 2 / ROW} width={() => chosen().width} height={() => chosen().height - 4 / ROW} radius={4}
            color="selected" border="text" borderWidth={1} opacity={() => chosen().music} />
          <Rect x={sweep.startX.min(sweep.x)} y={sweep.startY.min(sweep.y)} width={sweep.dx.abs()} height={sweep.dy.abs()} radius={4}
            color="fill" border="text-muted" borderWidth={1} opacity={sweep.active} />
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
                const step = (ROW - 24) / shown;
                const height = clamp(step, 2, 4);
                const bottom = ROW - 12 - ((shown - range) / 2) * step;
                return { lowest, step, height, bottom };
              });
              return (
                <>
                  <Rect x={-1e6} y={index} width={2e6} height={space.pointY} color="border" />
                  <For each={notes}>
                    {(note) => (
                      <Rect x={note.start} width={space.pointX.negate().plus(note.length).max(space.pointX)} color={() => colourOf(each)} opacity={() => (muted(each) ? DIM : 1)}
                        y={() => {
                          const { bottom, lowest, step, height } = miniature();
                          return index() + (bottom - (note.pitch - lowest + 0.5) * step - height / 2) / ROW;
                        }}
                        height={() => miniature().height / ROW} />
                    )}
                  </For>
                </>
              );
            }}
          </For>
          <Rect x={-1e6} y={() => rows().length} width={2e6} height={space.pointY} color="border" />
          <Show when={() => silent() && rows().length > 0 && memory.chosen === null}>
            {() => [
              <Rect x={() => space.left.max(0).plus(space.pointX.times(6))} y={0.5 - 12 / ROW} width={space.pointX.times(HINT.length * LABEL_CHAR + 12)} height={24 / ROW} radius={4} color="background" />,
              <Label x={() => space.left.max(0).plus(space.pointX.times(12))} y={0.5 - 8 / ROW} text={HINT} color="text-muted" />,
            ]}
          </Show>
          <Line from={() => [playhead, 0] as const} to={() => [playhead, rows().length] as const} color="text" width={1} />
          <QuietButton space={space} id="chosen-add-bar" x={buttonsX} y={buttonsY} text="+ Bar" width={44} shown={() => chosen().whole} onClick={() => addBar(page, "after")} />
          <QuietButton space={space} id="chosen-remove" x={() => buttonsX().plus(space.pointX.times(48))} y={buttonsY} text="Remove" width={56} shown={() => chosen().whole * (1 - chosen().all)}
            onClick={() => clearChosen(page, true)} />
          <QuietButton space={space} id="chosen-more" x={() => buttonsX().plus(space.pointX.times(108 * chosen().whole))} y={buttonsY} text="More" width={44} shown={() => chosen().whole + chosen().music}
            onClick={() => void choiceMenu(page)} />
        </Space>
        <Show when={() => rows().length === 0}>{() => <EmptyPiece page={page} />}</Show>
      </div>
      <div style={() => ({ height: returnsHeight() + LOW_ROW, keep: true })} />
    </>
  );
};

/** Over the rows while the piece has no instrument: what to start with, as the window's empty project says it. */
const EmptyPiece = ({ page }: { page: Page }) => (
  <div style={{ absolute: { top: 0, left: 0, right: 0, bottom: 0 }, align: "center", justify: "center", gap: "xs" }}>
    <Text variant="title">An empty piece</Text>
    <Text tone="muted">Add an instrument to write for, or ask the agent for one.</Text>
    <div style={{ paddingTop: "md" }}>
      <Button key="empty-instrument" label="Instrument" icon="plus" variant="subtle" size="md" onClick={() => addTrack(page, 0)} />
    </div>
  </div>
);
