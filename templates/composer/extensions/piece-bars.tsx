import { For, Fragment, h, now, Rect, Space } from "./sdk";
import { LOW_ROW, ROW, STRIP, type Page } from "./piece-page";

// The bars of the open section: the strip over the rows, and a row per instrument under it.

/** The strip of the bars of the open section, over the rows. */
export const BarStrip = (_: { page: Page }) => <div style={{ height: STRIP, keep: true }} />;

/** A row per instrument, which the heads scroll with, and under them the room beside the returns and the master. */
export const Rows = ({ page }: { page: Page }) => {
  const { rows, returnsHeight } = page;
  const space = now.space("rows");
  return (
    <>
      <Space key="rows" grow unit={{ y: ROW }} extent={() => ({ y: [0, rows().length + 1] })}>
        <For each={rows}>{(_, index) => <Rect x={-1e6} y={index} width={2e6} height={space.pointY} color="border" />}</For>
        <Rect x={-1e6} y={() => rows().length} width={2e6} height={space.pointY} color="border" />
      </Space>
      <div style={() => ({ height: returnsHeight() + LOW_ROW, keep: true })} />
    </>
  );
};
