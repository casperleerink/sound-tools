import { Button, For, h, Input, Label, memo, signal, Space, Text, Toggle, untrack, type Accessor, type Turn } from "./sdk";
import type { Piece } from "./piece";
import { addSection, deleteSection, duplicateSection, joinSection, moveSection, renameSection, setBars, setLength } from "./piece-edits";
import { choiceOf, toggleRepeat } from "./piece-choice";
import { partsOf } from "./piece-form";
import { barsIn, clamp, DIM, openAt, openOf, openSectionOf, rowLook, SECTION_HEAD, SECTIONS, secondsOf, timeText, type Page } from "./piece-page";

// The sections of the piece, one at a time open: the column that lists them, and the head of the
// open one with its length, time signature and tempo.

const SECTION_ROW = 36;
/** The note values a time signature takes. */
const NOTES = [1, 2, 4, 8, 16, 32];

const idsOf = (piece: Pick<Piece, "state">) => piece.state.sections.map((section) => section.id);
/** The bar numbers of a section in the piece: `5–12`, or `5` for one bar. */
const numbersOf = (place: { bar: number; bars: number }) => (place.bars === 1 ? `${place.bar}` : `${place.bar}–${place.bar + place.bars - 1}`);

/** Opens the section before or after the open one. */
export function stepSection(piece: Piece, by: -1 | 1) {
  const ids = idsOf(piece);
  const next = ids[ids.indexOf(openSectionOf(piece)?.id ?? "") + by];
  if (next) openAt(piece, next);
}
/** Moves the section `id` one place earlier or later. */
export function moveSectionBy(piece: Piece, id: string, by: -1 | 1) {
  const ids = idsOf(piece);
  const at = ids.indexOf(id);
  if (at < 0 || at + by < 0 || at + by >= ids.length) return;
  moveSection(piece, id, by < 0 ? ids[at - 1]! : (ids[at + 2] ?? null));
}
/** Adds a section after the open one and opens it, its name typed in the list or in the head. */
export function addSectionAfter(piece: Piece, where: "list" | "head") {
  const id = addSection(piece, openSectionOf(piece)?.id ?? null);
  openAt(piece, id);
  piece.memory.naming = { id, in: where };
}
/** Deletes the section `id`: the one after it opens in its place, or the one before. */
function removeSection(piece: Piece, id: string) {
  const ids = idsOf(piece);
  const next = ids[ids.indexOf(id) + 1] ?? ids[ids.indexOf(id) - 1];
  const wasOpen = openSectionOf(piece)?.id === id;
  deleteSection(piece, id);
  if (wasOpen && next) openAt(piece, next);
}
/** Joins the section `id` with the next: open, either stays open as the joined one. */
function joinNext(piece: Piece, id: string) {
  const ids = idsOf(piece);
  const next = ids[ids.indexOf(id) + 1];
  if (next === undefined) return;
  const wasOpen = openSectionOf(piece)?.id === next;
  joinSection(piece, id);
  if (wasOpen) {
    piece.memory.open = id;
    piece.memory.chosen = null;
  }
}

async function sectionMenu(piece: Piece, id: string) {
  if (openSectionOf(piece)?.id !== id) openAt(piece, id);
  const ids = idsOf(piece);
  const at = ids.indexOf(id);
  const last = at === ids.length - 1;
  const picked = await piece.openMenu([
    { label: "Rename", shortcut: "piece.rename_section" },
    { label: "Duplicate" },
    { label: "Move earlier", shortcut: "piece.move_section_earlier", disabled: at === 0, separator: true },
    { label: "Move later", shortcut: "piece.move_section_later", disabled: last },
    { label: "Join with the next", disabled: last },
    { label: "Delete", disabled: ids.length <= 1, separator: true },
  ]);
  if (picked === 0) piece.memory.naming = { id, in: "list" };
  else if (picked === 1) {
    const copy = duplicateSection(piece, id);
    if (copy) openAt(piece, copy);
  } else if (picked === 2 || picked === 3) moveSectionBy(piece, id, picked === 2 ? -1 : 1);
  else if (picked === 4) joinNext(piece, id);
  else if (picked === 5) removeSection(piece, id);
}

/** Names the section being named; no name keeps the one it has. */
function rename(piece: Piece, id: string, name: string) {
  piece.memory.naming = null;
  renameSection(piece, id, name);
}
const dropOn = (piece: Piece, carried: string | null, before: string | null) => {
  if (carried?.startsWith("section:")) moveSection(piece, carried.slice(8), before);
};

/**
 * The column of the sections: a row each, with its bar numbers, which opens it; a double click
 * names it, a right press opens its menu, a drag onto another moves it there, onto Add section
 * to the end. A green dot marks the one that plays; the whole piece at its foot.
 */
export const SectionsList = ({ page }: { page: Page }) => {
  const { sections, openId, open, memory, form } = page;
  const whole = memo(() => {
    const bars = sections().flatMap(barsIn);
    return `${bars.length} ${bars.length === 1 ? "bar" : "bars"} · ${timeText(secondsOf(bars))}`;
  });
  const naming = (id: string) => memory.naming?.in === "list" && memory.naming.id === id;
  return (
    <div style={{ width: SECTIONS - 1, keep: true }}>
      <div style={{ grow: true, scroll: "y", paddingTop: "sm" }}>
        <For each={() => sections().map((section) => section.id)}>
          {(id) => (
            <div key={`section-${id}`} tabStop carry={`section:${id}`} onDrop={({ carried }) => dropOn(page, carried, id)}
              onPress={({ button }) => button === "right" && void sectionMenu(page, id)}
              onClick={({ clicks }) => (clicks === 2 ? (memory.naming = { id, in: "list" }) : open(id))}
              style={() => ({ height: SECTION_ROW, direction: "row", align: "center", gap: "sm", paddingX: "lg", ...rowLook(openId() === id) })}>
              <div style={() => ({ absolute: { left: 5, top: (SECTION_ROW - 6) / 2 }, width: 6, height: 6, radius: "full", background: "green", opacity: memory.playing === id ? 1 : 0 })} />
              {() =>
                naming(id) ? (
                  <Input key={`section-name-${id}`} value={() => sections().find((section) => section.id === id)?.name ?? ""} width={120} size="sm" focus
                    onCommit={(name) => rename(page, id, name)} onCancel={() => (memory.naming = null)} />
                ) : (
                  <Text variant="heading" style={{ grow: true, truncate: true }}>{() => sections().find((section) => section.id === id)?.name ?? ""}</Text>
                )
              }
              <Text variant="label" tone="muted" style={{ keep: true }}>{() => {
                const place = form().places[id];
                return place ? numbersOf(place) : "";
              }}</Text>
            </div>
          )}
        </For>
        <div style={{ height: SECTION_ROW, direction: "row", align: "center", paddingX: "sm" }} onDrop={({ carried }) => dropOn(page, carried, null)}>
          <Button key="add-section" label="Add section" icon="plus" variant="ghost" color="text-secondary" size="sm" shortcut="piece.add_section" tooltip="Add a section after the open one"
            onClick={() => addSectionAfter(page, "list")} />
        </div>
      </div>
      <div style={{ paddingX: "lg", paddingY: "md", keep: true }}>
        <Text variant="label" tone="muted">{whole}</Text>
      </div>
    </div>
  );
};

/**
 * A number of the section head in title type, as a number box: a drag up or down steps it and
 * changes the piece once, as it is let go, so a bar made shorter and longer again in one drag
 * keeps its notes. A click types it; the arrows step it. `steps` gives the value `by` steps from
 * one, and with 0 the nearest it takes. Dim while the bars it is for differ.
 */
const Amount = ({ id, value, steps, set, width, per = 10, align = "left", mixed = () => false }: {
  id: string; value: Accessor<number>; steps: (from: number, by: number) => number; set: (value: number) => void; width: number; per?: number; align?: "left" | "right"; mixed?: Accessor<boolean>;
}) => {
  const [dragged, drag] = signal<number | null>(null);
  const [typing, type] = signal(false);
  let pressed = 0;
  const onTurn = (turn: Turn) => {
    if (!("moved" in turn)) return;
    const next = steps(turn.held ? pressed : untrack(value), turn.moved);
    if (turn.held) drag(next);
    else if (next !== untrack(value)) set(next);
  };
  const release = () => {
    const next = untrack(dragged);
    drag(null);
    if (next !== null && next !== untrack(value)) set(next);
  };
  const typed = (text: string) => {
    type(false);
    const number = Number.parseFloat(text);
    if (Number.isFinite(number)) set(steps(number, 0));
  };
  return (
    <div style={{ keep: true }} keys={["enter"]} onKey={() => type(true)}>
      {() =>
        typing() ? (
          <Input key={`${id}-typed`} value={String(untrack(value))} width={Math.max(width + 24, 48)} size="lg" focus onCommit={typed} onCancel={() => type(false)} />
        ) : (
          <Space key={id} width={width} height={32} cursor="ns-resize" drag={{ y: { onTurn, step: 1, per } }}
            onPress={() => (pressed = untrack(value))} onRelease={release} onClick={() => type(true)}>
            <Label x={align === "right" ? width : 0} y={6} align={align} variant="title" color="text" text={() => String(dragged() ?? value())}
              opacity={() => (mixed() && dragged() === null ? DIM : 1)} />
          </Space>
        )
      }
    </div>
  );
};

/** A caption over what it names, in the section head. */
const Field = ({ caption, children }: { caption: string; children?: h.JSX.Element | h.JSX.Element[] }) => (
  <div style={{ keep: true, gap: 2 }}>
    <Text variant="label" tone="muted">{caption}</Text>
    <div style={{ direction: "row", align: "center", height: 32 }}>{children}</div>
  </div>
);

/**
 * The head of the open section: its name, which a double click types, where it is in the piece
 * and how long it plays; its length in bars; and, after the bars they are for, the chosen ones or all, their time
 * signature and tempo, the first bar's dim where they differ; and a toggle that repeats them.
 */
export const SectionHead = ({ page }: { page: Page }) => {
  const { openSection, openId, place, bars, memory, clock } = page;
  /** The bars the time signature and tempo are for: the chosen ones, else all. */
  const target = memo(() => {
    const choice = choiceOf(page);
    return choice ? { from: choice.from, to: choice.to, chosen: true } : { from: 0, to: bars().length - 1, chosen: false };
  });
  const targeted = () => bars().slice(target().from, target().to + 1);
  const first = () => targeted()[0] ?? { signature: "4/4", bpm: 120 };
  const differ = (of: (bar: { signature: string; bpm: number }) => number) => () => targeted().some((bar) => of(bar) !== of(first()));
  const [beatsOf, noteOf] = [(bar: { signature: string }) => partsOf(bar.signature)[0], (bar: { signature: string }) => partsOf(bar.signature)[1]];
  const caption = () => {
    const { from, to, chosen } = target();
    const bar = place()?.bar ?? 1;
    return !chosen ? "All bars" : from === to ? `Bar ${bar + from}` : `Bars ${bar + from}–${bar + to}`;
  };
  const change = (bar: { signature?: string; bpm?: number }) => {
    const id = openId();
    if (id !== null) setBars(page, id, target().from, target().to, bar);
  };
  const lengthTo = (count: number) => {
    const id = openId();
    if (id === null) return;
    setLength(page, id, count);
    const chosen = memory.chosen;
    if (chosen && chosen.from >= count) memory.chosen = null;
    else if (chosen && chosen.to >= count) memory.chosen = { ...chosen, to: count - 1 };
  };
  const naming = () => memory.naming?.in === "head" && memory.naming.id === openId();
  return (
    <div style={{ height: SECTION_HEAD, direction: "row", align: "center", gap: "2xl", paddingX: "lg", keep: true }}>
      <div style={{ grow: true, minWidth: 0, justify: "center" }} onClick={({ clicks }) => {
        const id = openId();
        if (clicks === 2 && id !== null) memory.naming = { id, in: "head" };
      }}>
        {() => {
          const id = openId();
          return naming() && id !== null ? (
            <Input key={`head-name-${id}`} value={() => openSection()?.name ?? ""} width={280} size="lg" focus onCommit={(name) => rename(page, id, name)} onCancel={() => (memory.naming = null)} />
          ) : (
            <Text variant="display" style={{ truncate: true }}>{() => openSection()?.name ?? ""}</Text>
          );
        }}
        <Text variant="label" tone="muted">
          {() => {
            const at = place();
            return at ? `${at.bars === 1 ? "Bar" : "Bars"} ${numbersOf(at)} · ${timeText(secondsOf(bars()))}` : "";
          }}
        </Text>
      </div>
      <Field caption="Length">
        <Amount id="section-length" value={() => bars().length} steps={(from, by) => clamp(Math.round(from + by), 1, 999)} set={lengthTo} width={36} align="right" />
        <Text tone="muted" style={{ paddingLeft: "sm" }}>{() => (bars().length === 1 ? "bar" : "bars")}</Text>
      </Field>
      <div style={{ width: 1, height: 40, background: "border", keep: true }} />
      <Text key="section-scope" variant="heading" tone={() => (target().chosen ? "text" : "muted")} style={{ keep: true }}>{caption}</Text>
      <Field caption="Time signature">
        <Amount id="section-beats" value={() => beatsOf(first())} mixed={differ(beatsOf)} width={20} align="right"
          steps={(from, by) => clamp(Math.round(from + by), 1, 32)} set={(beats) => change({ signature: `${beats}/${noteOf(first())}` })} />
        <Text variant="title" tone="muted" style={{ paddingX: "xs" }}>/</Text>
        <Amount id="section-note" value={() => noteOf(first())} mixed={differ(noteOf)} width={20}
          steps={(from, by) => {
            const nearest = NOTES.reduce((best, each) => (Math.abs(each - from) < Math.abs(best - from) ? each : best), 4);
            return NOTES[clamp(NOTES.indexOf(nearest) + by, 0, NOTES.length - 1)]!;
          }}
          set={(note) => change({ signature: `${beatsOf(first())}/${note}` })} />
      </Field>
      <Field caption="Tempo">
        <Amount id="section-tempo" value={() => first().bpm} mixed={differ((bar) => bar.bpm)} width={40} per={2} align="right"
          steps={(from, by) => clamp(Math.round((from + by) * 10) / 10, 10, 1000)} set={(bpm) => change({ bpm })} />
        <Text tone="muted" style={{ paddingLeft: "sm" }}>bpm</Text>
      </Field>
      <div style={{ paddingTop: 18, keep: true }}>
        <Toggle key="section-repeat" label="Repeat" on={() => clock.repeat !== null}
          tooltip={() => (clock.repeat !== null ? "Play on" : target().chosen ? "Repeat the chosen bars" : "Repeat the section")}
          shortcut="piece.repeat" onClick={() => toggleRepeat(page)} />
      </div>
    </div>
  );
};
