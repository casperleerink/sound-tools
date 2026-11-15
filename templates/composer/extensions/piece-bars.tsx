import { Arc, Button, Circle, For, Fragment, h, Label, Line, memo, now, Rect, Show, Space, Text, untrack, type Drag, type Live, type Num, type Press, type SpaceValues } from "./sdk";
import { addBar, askSignature, askSlide, askTempo, choiceMenu, choiceOf, chooseRows, clearChosen, fermataMenu, moveFermata } from "./piece-choice";
import { moveSection } from "./piece-edits";
import { beatOf, countOf, endOf, formText, tempoOf, type Bar } from "./piece-form";
import { ACROSS, AFTER, barAtTick, chooseBars, clamp, colourOf, DIM, HEADER, LEAD, LOW_ROW, MARKS, muted, ROW, ZOOM, type Page } from "./piece-page";
import { addSectionAtEnd, chooseSection, nameSection, sectionMenu } from "./piece-sections";
import { addTrack } from "./piece-tracks";

// The whole piece on one line, as a score's continuous view: over the rows a band of marks, with
// the section names, the time signatures and tempos where they change, a line where the tempo
// slows down or speeds up, the bar numbers and the fermatas; under it a row of notes per instrument. Across, a unit is a tick; the rows scroll and zoom, the band
// follows them.

/** Where the lines of the band start: the section names, the two digits of a time signature, a tempo, the bar numbers. */
const [NAME_Y, TOP_DIGIT, BOTTOM_DIGIT, TEMPO_Y, NUMBER_Y] = [7, 28, 41, 35, 63];
/** Where the double line at a section's end starts, under the names; and the bar-number line, where chosen whole bars show, as a score's bar track. */
const [LINES_Y, TRACK_Y] = [28, NUMBER_Y - 3];
/** The points a bar's number needs before the next one, and before a fermata on its bar; and which bars keep theirs as the room gets less, as a score numbers every fifth or tenth. */
const NUMBER_ROOM = 44;
const NUMBERED = [1000, 500, 100, 50, 10, 5, 1];
/** About how many points a character takes: of a label, of a bold label, of a bold digit of a time signature. */
const [LABEL_CHAR, BOLD_LABEL_CHAR, DIGIT] = [5.6, 6.6, 8.4];
/** A fermata, in the line of the bar numbers: where the middle of its arc is, and its radius. */
const [FERMATA_Y, FERMATA] = [NUMBER_Y + 9, 7];
const HINT = "Double-click a bar to write notes";
/** Where the piece opens: whole when a bar of 4/4 keeps this many points, else at zoom 1. */
const NARROWEST = 40;
const [FIT_BEFORE, FIT_AFTER] = [12, 120];
/** How wide the two buttons on chosen whole bars are together, and the points they keep from an edge or the last row. */
const [BUTTONS, GAP] = [104, 6];

/** 1 while `ticks` give at least `points` of room at the zoom it shows: the window works it out as it zooms. */
const fits = ({ space }: Page, ticks: number, points: number) => space.pointX.le(ticks / points);
const not = (value: Live) => value.negate().plus(1);
/** A live 0, for a mark with no time signature. */
const NONE = now.tick.times(0);

const said = (bar: Bar, bpm = bar.bpm) => `${beatOf(bar.signature).sign} = ${tempoOf({ ...bar, bpm })}`;

/**
 * A mark at a bar where the time signature or the tempo changes: the forms it shows, from the
 * longest, each with the points it needs. Bars that slow down or speed up along one straight line
 * have one mark, `rit.` or `accel.`, and the bar after them says its tempo again.
 */
function marksOf({ all, end }: Page) {
  const { bars, starts } = all();
  const slides = (index: number) => bars[index]?.to !== undefined;
  const rises = (index: number) => endOf(bars[index]!) > bars[index]!.bpm;
  /** Whether a bar slides on from the bar before it: from the tempo that one ends at, the same way. */
  const goesOn = (index: number) => slides(index) && slides(index - 1) && bars[index - 1]!.to === bars[index]!.bpm && rises(index) === rises(index - 1);
  const tempoAt = (index: number) => index === 0 || (slides(index) ? !goesOn(index) : slides(index - 1) || said(bars[index - 1]!) !== said(bars[index]!));
  const signatureAt = (index: number) => index === 0 || bars[index - 1]!.signature !== bars[index]!.signature;
  /** The bars the time signature or the tempo of the bar `index` stands for: from the last mark of its kind up to the next. */
  const runOf = (index: number, marked: (index: number) => boolean) => {
    const next = bars.findIndex((_, at) => at > index && marked(at));
    return { from: Math.max(bars.findLastIndex((_, at) => at <= index && marked(at)), 0), to: next < 0 ? bars.length - 1 : next - 1 };
  };
  const changes = bars.flatMap((bar, index) => (signatureAt(index) || tempoAt(index) ? [{ index, tick: starts[index]!, bar, signature: signatureAt(index), tempo: tempoAt(index) }] : []));
  return changes.map((change, at) => {
    const { bar, index } = change;
    const [beats, note] = bar.signature.split("/");
    const signatureWidth = Math.max(beats!.length, note!.length) * DIGIT;
    const lead = change.signature ? signatureWidth + 8 : 0;
    const tempoRun = runOf(index, tempoAt);
    const [last, next] = [bars[tempoRun.to]!, bars[tempoRun.to + 1]];
    const sliding = change.tempo && slides(index);
    const word = rises(index) ? "accel." : "rit.";
    // A slide that does not start where the bar before it ends says the tempo it starts at.
    const jumps = index === 0 || endOf(bars[index - 1]!) !== bar.bpm;
    const texts = !change.tempo ? [] : sliding ? [...(jumps ? [`${said(bar)}  ${word}`] : []), word] : [said(bar), String(tempoOf(bar))];
    const forms = [
      ...texts.map((tempo) => ({ tempo, width: 6 + lead + tempo.length * LABEL_CHAR + 8 })),
      // The time signature alone, with a dot for a tempo that changes there.
      ...(change.signature ? [{ tempo: "", width: change.tempo ? 6 + lead + 6 : 6 + signatureWidth + 4 }] : []),
    ];
    // The tempo a slide reaches is said at its end, unless the bar after it says it.
    const reached = next && !slides(tempoRun.to + 1) && next.bpm === endOf(last) ? "" : String(tempoOf({ ...last, bpm: endOf(last) }));
    return {
      ...change, signatureWidth, lead, forms, beats: beats!, note: note!, tempoRun,
      room: (changes[at + 1]?.tick ?? end()) - change.tick,
      signatureRun: runOf(index, signatureAt),
      slide: sliding ? { end: starts[tempoRun.to + 1]!, reached } : null,
      // What the mark says of the tempo once its bar is off the left edge, where it stays: also when the tempo did not change at it.
      edgeTempo: slides(index) ? word : said(bar),
      slides: slides(index),
    };
  });
}

/** The sections, each from the tick it starts at to where it ends. */
const sectionsOf = ({ sections, form }: Page) =>
  sections().flatMap((section) => {
    const place = form().places[section.id];
    return place ? [{ id: section.id, name: section.name.trim() || "Section", from: place.start, to: place.start + place.length }] : [];
  });

/** The chosen bars, from where the first starts to where the last ends, in ticks, and the rows of the chosen instruments: all 0 while nothing is chosen. */
const chosenSpan = (page: Page) => {
  const { starts, rows } = page;
  const choice = choiceOf(page);
  if (!choice) return { x: 0, width: 0, whole: 0, music: 0, all: 0, top: 0, height: 0 };
  const [from, to] = [starts()[choice.from]!, starts()[choice.to + 1]!];
  const span = { x: from, width: to - from, all: choice.from === 0 && choice.to === choice.all.bars.length - 1 ? 1 : 0 };
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
const QuietButton = ({ space, id, x, y, text, width, shown, onClick, large = false }: {
  space: SpaceValues; id: string; x: () => Live; y: () => Num; text: string; width: number; shown: () => number; onClick: () => void; large?: boolean;
}) => {
  const height = space.pointY.times(large ? 24 : 20);
  return (
    <>
      <Rect x={x} y={y} width={space.pointX.times(width)} height={height} radius={4} color="background" opacity={shown} />
      <Rect key={id} x={x} y={y} width={() => space.pointX.times(width).times(shown())} height={height} radius={4} color="fill" hover="fill-strong" cursor="pointer" opacity={shown} onClick={onClick} />
      <Label x={() => x().plus(space.pointX.times(width / 2))} y={() => space.pointY.times(2).plus(y())} align="center" variant={large ? "title" : "label"} text={text} color="text-secondary" opacity={shown} />
    </>
  );
};

/**
 * The band over the rows. A press on a bar chooses it whole, all instruments, shift-press or a
 * drag a range, and puts the playhead there; a right press opens the menu of the chosen bars. A
 * click on a time signature, a tempo or a slide types it for the bars it stands for, a right press
 * chooses its bar as one over it does. A click on a fermata opens how long it holds, a drag moves
 * it to another count. A section's name chooses its bars, a double click names it, a right press
 * opens its menu, a drag moves it.
 */
export const Marks = ({ page }: { page: Page }) => {
  const { memory, all, starts, end, barAt, space, sections } = page;
  const band = now.space("marks");
  const marks = memo(() => marksOf(page));
  const parts = memo(() => sectionsOf(page));
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
    const room = index === 0 || chosen ? not(NONE) : fits(page, (starts()[index + apart] ?? tick + apart * 4 * 960) - tick, NUMBER_ROOM);
    // Not under the fermata of its bar.
    const fermata = all().bars[index]?.fermata;
    return fermata ? room.times(fits(page, (fermata.beat - 0.5) * countOf(all().bars[index]!.signature), 28)) : room;
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
  const fermataSign = (x: Num | (() => Num), opacity: Num | (() => Num)) => [
    <Arc x={x} y={FERMATA_Y} radius={FERMATA} from={-0.25} to={0.25} color="text" width={1.5} opacity={opacity} />,
    <Circle x={x} y={FERMATA_Y - 2} radius={1.5} color="text" opacity={opacity} />,
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
    } else if (shift && at !== null) chooseBars(page, Math.min(at.from, bar), Math.max(at.to, bar));
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
      if (before !== id && before !== (parts()[parts().findIndex((part) => part.id === id) + 1]?.id ?? null)) moveSection(page, id, before);
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

  return (
    <Space key="marks" height={MARKS} unit={{ x: ACROSS }} extent={() => ({ x: [-LEAD, end() + AFTER] })} zoom={{ x: ZOOM }} follow={{ x: "rows" }}
      onPress={press} drag={sweepDrag} cursor="pointer">
      <Rect x={() => chosen().x} y={TRACK_Y} width={() => chosen().width} height={MARKS - TRACK_Y - 2} radius={4} color="selected" border="text" borderWidth={1} opacity={() => chosen().whole} />
      <Rect x={sweep.startX.min(sweep.x)} y={TRACK_Y} width={sweep.dx.abs()} height={MARKS - TRACK_Y - 2} radius={4} color="fill" border="text-muted" borderWidth={1} opacity={sweep.active} />
      <For each={parts}>
        {(part) => {
          // The name boxed, or its first letter as a rehearsal mark, or a dot: never nothing.
          // The last one ends where "+ Section" starts.
          const room = () => (parts().find((each) => each.from > part.from)?.from ?? end()) - part.from;
          const widths = [part.name.length * BOLD_LABEL_CHAR + 26, 32];
          const shows = [() => fits(page, room(), widths[0]!), () => fits(page, room(), widths[1]!).times(not(fits(page, room(), widths[0]!))), () => not(fits(page, room(), widths[1]!))];
          // Its name stays at the left edge while the section goes on past it, up to its end.
          const nameX = () => band.left.max(part.from).min(space.pointX.times(-widths[0]!).plus(part.to)).max(part.from);
          // Bright while all its bars, and only they, are chosen.
          const colour = () => {
            const [at, bars] = [memory.chosen, all().bars];
            return at?.rows === null && bars[at.from]?.id === part.id && bars[at.to]?.id === part.id && bars[at.from]?.at === 0 && bars[at.to + 1]?.id !== part.id ? "text" : "text-secondary";
          };
          return [
            <Rect x={() => space.pointX.times(-3).plus(part.to)} y={LINES_Y} width={space.pointX} height={MARKS - LINES_Y} color="border-strong" />,
            <Rect x={part.to} y={LINES_Y} width={space.pointX} height={MARKS - LINES_Y} color="border-strong" />,
            <Label x={() => nameX().plus(space.pointX.times(6))} y={NAME_Y} opacity={shows[0]} text={() => [{ text: ` ${part.name} `, color: "text", weight: "semibold", border: colour() }]} />,
            <Label x={() => space.pointX.times(6).plus(part.from)} y={NAME_Y} opacity={shows[1]} text={() => [{ text: ` ${part.name.slice(0, 1)} `, color: "text", weight: "semibold", border: colour() }]} />,
            <Circle x={() => space.pointX.times(8).plus(part.from)} y={NAME_Y + 9} radius={2.5} color="text-secondary" opacity={shows[2]} />,
            <Rect key={`section-${part.id}`} x={() => nameX().plus(space.pointX.times(4))} y={NAME_Y - 2} height={22} color="none" cursor="grab"
              width={() => space.pointX.times(widths[0]!).times(shows[0]()).plus(space.pointX.times(widths[1]!).times(shows[1]())).plus(space.pointX.times(12).times(shows[2]()))}
              drag={carryDrag} onPress={pressName(part.id)} onClick={clickName(part.id)} />,
          ];
        }}
      </For>
      <Rect x={() => landing().minus(space.pointX)} y={0} width={space.pointX.times(2)} height={MARKS} color="lavender" opacity={() => carry.active.times(carry.dx.abs().gt(0)).times(not(landsInPlace()))} />
      <For each={marks}>
        {(mark) => {
          // The longest form that has room before the next mark, else a dot.
          const shown = mark.forms.map((form, index) => {
            const room = fits(page, mark.room, form.width);
            const before = mark.forms[index - 1];
            return before ? room.times(not(fits(page, mark.room, before.width))) : room;
          });
          const last = mark.forms.at(-1)!;
          const dot = not(fits(page, mark.room, last.width));
          const signatureShows = mark.signature ? not(dot) : NONE;
          // Once its bar is off the left edge, a mark stays there up to the next one, and says the time signature and the tempo both, as a score's continuous view keeps them: while it has the room.
          const [signatureLead, next] = [mark.signatureWidth + 8, mark.tick + mark.room];
          const edgeWidth = 6 + signatureLead + Math.max(mark.forms[0]?.tempo.length ?? 0, mark.edgeTempo.length) * LABEL_CHAR + 8;
          const stays = fits(page, mark.room, edgeWidth + 24);
          const atEdge = band.left.gt(mark.tick).times(stays);
          const along = (from: Live, to: Live) => to.max(from).minus(from).times(stays).plus(from);
          const signatureX = () => along(now.tick.times(0).plus(mark.tick), band.left.min(space.pointX.times(-edgeWidth).plus(next)));
          const tempoX = () =>
            along(space.pointX.times(6 + mark.lead).plus(mark.tick), band.left.plus(space.pointX.times(6 + signatureLead)).min(space.pointX.times(-(edgeWidth - 6 - signatureLead)).plus(next)));
          const left = (points: number) => () => space.pointX.times(points).plus(signatureX());
          // A click types for the bars the mark stands for, which it chooses, so they show; a right press chooses the bar of the mark, as one on the band over it does.
          const pressMark = (run: { from: number; to: number }, type: () => unknown) => (event: Press) => {
            if (event.button !== "left") return press({ ...event, x: mark.tick });
            chooseBars(page, run.from, run.to);
            void type();
          };
          const signatureOpacity = mark.signature ? signatureShows : atEdge;
          const digit = (text: string, y: number) => (
            <Label x={left(6 + mark.signatureWidth / 2)} y={y} align="center" variant="body" text={[{ text, weight: "semibold" }]} color="text" opacity={signatureOpacity} />
          );
          const typeTempo = pressMark(mark.tempoRun, () => (mark.slides ? askSlide : askTempo)(page, mark.tempoRun));
          // The line of a slide, from its word to where it ends, with the tempo it reaches.
          const slide = (() => {
            if (!mark.slide) return [];
            const { end, reached } = mark.slide;
            // The word may have its start tempo before it or not: the line starts after the form that shows.
            const words = mark.forms.filter((form) => form.tempo !== "").map((form, index) => ({ width: form.width, shows: shown[index]! }));
            const number = reached.length * LABEL_CHAR + (reached ? 10 : 0);
            const room = (points: number) => words.reduce((sum, word) => sum.plus(word.shows.times(fits(page, end - mark.tick, word.width + points))), NONE);
            // The tempo it reaches shows before its line does: with little room, the word and the number.
            const [lineShows, numberShows] = [room(number + (reached ? 14 : 12)), reached ? room(number + 2) : NONE];
            const word = words.reduce((sum, each) => sum.plus(each.shows.times(each.width - mark.lead - 10)), NONE);
            const lineEnd = () => space.pointX.times(-number).times(numberShows).plus(space.pointX.times(-5)).plus(end);
            const lineStart = () => space.pointX.times(word).plus(tempoX()).min(lineEnd());
            const y = TEMPO_Y + 8;
            return [
              <Line from={() => [lineStart(), y] as const} to={() => [lineEnd(), y] as const} color="text-muted" width={1} opacity={lineShows} />,
              <Label x={() => space.pointX.times(-5).plus(end)} y={TEMPO_Y} align="right" text={reached} color="text-muted" opacity={numberShows} />,
              <Rect key={`slide-${mark.index}`} x={lineStart} y={TOP_DIGIT - 2} width={() => lineStart().negate().plus(end).times(lineShows)} height={30} color="none" cursor="pointer" onPress={typeTempo} />,
            ];
          })();
          const tempoBox = (key: string, width: number, shows: Num) => (
            <Rect key={key} x={() => space.pointX.times(-4).plus(tempoX())} y={TOP_DIGIT - 2} width={space.pointX.times(width).times(shows)} height={30} color="none" cursor="pointer" onPress={typeTempo} />
          );
          return [
            digit(mark.beats, TOP_DIGIT), digit(mark.note, BOTTOM_DIGIT),
            <Rect key={`signature-${mark.index}`} x={left(2)} y={TOP_DIGIT - 2} width={() => space.pointX.times(mark.signatureWidth + 8).times(signatureOpacity)} height={30} color="none" cursor="pointer"
              onPress={pressMark(mark.signatureRun, () => askSignature(page, mark.signatureRun))} />,
            ...slide,
            ...(mark.tempo
              ? mark.forms.flatMap((form, index) => [
                  form.tempo === ""
                    ? <Circle x={() => space.pointX.times(2.5).plus(tempoX())} y={TEMPO_Y + 8} radius={2} color="text-muted" opacity={shown[index]} />
                    : <Label x={tempoX} y={TEMPO_Y} text={form.tempo} color="text-secondary" opacity={shown[index]} />,
                  tempoBox(`tempo-${mark.index}-${index}`, form.width - mark.lead - 4, shown[index]!),
                ])
              : [<Label x={tempoX} y={TEMPO_Y} text={mark.edgeTempo} color="text-secondary" opacity={atEdge} />, tempoBox(`tempo-${mark.index}-edge`, mark.edgeTempo.length * LABEL_CHAR + 8, atEdge)]),
            <Circle x={left(8)} y={TEMPO_Y + 8} radius={2} color="text-muted" opacity={dot} />,
          ];
        }}
      </For>
      <For each={() => starts().slice(0, -1)}>
        {(tick, index) => (
          <Label x={() => space.pointX.times(4).plus(tick)} y={NUMBER_Y} text={() => String(index() + 1)} color={() => (inChosen(index()) ? "text" : "text-muted")} opacity={() => numberShows(index())} />
        )}
      </For>
      <For each={fermatas}>
        {(fermata) => [
          ...fermataSign(() => fermataX(fermata), 1),
          <Rect key={`fermata-${fermata.index}`} x={() => space.pointX.times(-9).plus(fermataX(fermata))} y={NUMBER_Y} width={space.pointX.times(18)} height={MARKS - NUMBER_Y} color="none" cursor="pointer"
            drag={holdDrag} onPress={pressFermata(fermata.index)} onClick={() => void fermataMenu(page, fermata.index)} />,
        ]}
      </For>
      {fermataSign(hold.x, () => hold.active.times(hold.dx.abs().gt(0)).times(0.5))}
      <QuietButton space={band} id="add-bar-end" x={after(10)} y={() => TOP_DIGIT + 4} text="+ Bar" width={72} shown={() => 1} onClick={() => addBar(page, "end")} />
      <QuietButton space={band} id="add-section" x={after(10)} y={() => NAME_Y - 2} text="+ Section" width={72} shown={() => 1} onClick={() => addSectionAtEnd(page)} />
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
  /** The bar lines inside sections; a section's ends are double. */
  const lines = memo(() => starts().filter((tick) => tick > 0 && !parts().some((part) => part.from === tick || part.to === tick)));
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
      chooseRows(page, Math.min(at.from, bar), Math.max(at.to, bar), names().slice(Math.min(...indexes), Math.max(...indexes) + 1));
    } else if (shift && at) chooseBars(page, Math.min(at.from, bar), Math.max(at.to, bar));
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
          <Rect x={() => chosen().x} y={0} width={() => chosen().width} height={() => rows().length} color="selected" opacity={() => chosen().whole} />
          <Rect x={() => chosen().x} y={() => chosen().top + 2 / ROW} width={() => chosen().width} height={() => chosen().height - 4 / ROW} radius={4}
            color="selected" border="text" borderWidth={1} opacity={() => chosen().music} />
          <Rect x={sweep.startX.min(sweep.x)} y={sweep.startY.min(sweep.y)} width={sweep.dx.abs()} height={sweep.dy.abs()} radius={4}
            color="fill" border="text-muted" borderWidth={1} opacity={sweep.active} />
          <For each={lines}>{(tick) => <Rect x={tick} y={0} width={space.pointX} height={() => rows().length} color="border" />}</For>
          <For each={parts}>
            {(part) => [
              <Rect x={() => space.pointX.times(-3).plus(part.to)} y={0} width={space.pointX} height={() => rows().length} color="border-strong" />,
              <Rect x={part.to} y={0} width={space.pointX} height={() => rows().length} color="border-strong" />,
            ]}
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
          <Show when={() => silent() && rows().length > 0}>
            {() => [
              <Rect x={() => space.left.max(0).plus(space.pointX.times(6))} y={0.5 - 12 / ROW} width={space.pointX.times(HINT.length * LABEL_CHAR + 12)} height={24 / ROW} radius={4} color="background" />,
              <Label x={() => space.left.max(0).plus(space.pointX.times(12))} y={0.5 - 8 / ROW} text={HINT} color="text-muted" />,
            ]}
          </Show>
          <Line from={() => [playhead, 0] as const} to={() => [playhead, rows().length] as const} color="text" width={1} />
          <QuietButton space={space} id="chosen-add-bar" x={buttonsX} y={buttonsY} text="+ Bar" width={44} shown={() => chosen().whole} onClick={() => addBar(page, "after")} />
          <QuietButton space={space} id="chosen-remove" x={() => buttonsX().plus(space.pointX.times(48))} y={buttonsY} text="Remove" width={56} shown={() => chosen().whole * (1 - chosen().all)}
            onClick={() => clearChosen(page, true)} />
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
