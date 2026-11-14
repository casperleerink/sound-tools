import { For, h, Text } from "./sdk";
import { rowLook, SECTION_HEAD, SECTIONS, secondsOf, timeText, type Page } from "./piece-page";

// The sections of the piece, one at a time open: the column that lists them and the head of the open one.

/** The column of the sections: a row each, which opens it. */
export const SectionsList = ({ page }: { page: Page }) => {
  const { sections, openId, open } = page;
  return (
    <div style={{ width: SECTIONS - 1, keep: true, paddingTop: "sm" }}>
      <For each={() => sections().map((section) => section.id)}>
        {(id) => (
          <div key={`section-${id}`} tabStop onClick={() => open(id)}
            style={() => ({ height: 36, direction: "row", align: "center", paddingX: "md", ...rowLook(openId() === id) })}>
            <Text variant="heading" style={{ truncate: true }}>{() => sections().find((section) => section.id === id)?.name ?? ""}</Text>
          </div>
        )}
      </For>
    </div>
  );
};

/** The head of the open section: its name, and where it is in the piece and how long it plays. */
export const SectionHead = ({ page }: { page: Page }) => {
  const { openSection, place, bars } = page;
  return (
    <div style={{ height: SECTION_HEAD, justify: "center", paddingX: "lg", keep: true }}>
      <Text variant="display">{() => openSection()?.name ?? ""}</Text>
      <Text variant="label" tone="muted">
        {() => {
          const at = place();
          return at ? `Bars ${at.bar}–${at.bar + at.bars - 1} · ${timeText(secondsOf(bars()))}` : "";
        }}
      </Text>
    </div>
  );
};

