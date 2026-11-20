import { h, type Child } from "../sdk";
import theme from "../theme.json";
import { positionOf, type Range } from "./value";

const [CELL, ROW, GAP] = [theme.size.cell, theme.size.row, 8];
/** The strip of a control above the drawing: as tall as a knob, so it lines up with the first row of cells. */
const STRIP = theme.size.knob;

/** How wide a display of `columns` cells is, for what draws in it. */
export const displayWidth = (columns: 4 | 6) => columns * CELL - GAP;
export const DISPLAY_HEIGHT = 2 * ROW;
/** The size of the drawing of a display, under its strip when it has one. */
export const drawingOf = (columns: 4 | 6 = 4, strip = false) => ({ w: displayWidth(columns), h: DISPLAY_HEIGHT - (strip ? STRIP : 0) });

/** A line of the scale at `at`, 0 to 1 from the left or from the bottom, with its faint word. */
export type Tick = { at: number; text?: string };

/** 20 Hz to 20 kHz evenly in octaves, as a response feed spreads it and a frequency knob turns. */
export const FREQUENCY: Range = { min: 20, max: 20000, scale: "log" };
/** The octaves above 20 Hz a response feed covers, for `units` across. */
export const OCTAVES: [number, number] = [0, Math.log2(1000)];
/** Where `value` is up a drawing whose top and bottom are `range`, 0 at the bottom, as `Curve` and `zero` take it. */
export const heightOf = (value: number, [top, bottom]: [number, number]) => (value - bottom) / (top - bottom);
/** 100, 1k and 10k across a display of {@link FREQUENCY}. */
export const FREQUENCY_TICKS: Tick[] = [[100, "100"], [1000, "1k"], [10000, "10k"]].map(([hz, text]) => ({ at: positionOf(hz as number, FREQUENCY), text: text as string }));
/** A line every `step` dB inside `range` [bottom, top] up a display, each with its number; 0 is left to the zero line. */
export const decibelTicks = ([bottom, top]: [number, number], step: number): Tick[] => {
  const ticks: Tick[] = [];
  for (let db = Math.ceil(bottom / step) * step; db <= top; db += step) if (db !== 0 && db !== bottom && db !== top) ticks.push({ at: (db - bottom) / (top - bottom), text: `${db > 0 ? "+" : ""}${db}`.replace("-", "−") });
  return ticks;
};

export type DisplayProps = {
  columns?: 4 | 6;
  /** A control above the drawing, such as the type of a filter: only the faint marks of the scale lie over the drawing. */
  strip?: Child;
  /** Keeps the room of the strip while it holds nothing, so the drawing does not jump. */
  keepStrip?: boolean;
  /** Lines down the drawing, their words at the top. */
  across?: readonly Tick[];
  /** Lines over the drawing, their words at the left. */
  up?: readonly Tick[];
  /** The 0 dB line, 0 to 1 from the bottom: a little brighter than the scale. */
  zero?: number;
  /** The line at its foot, which says only what moves. */
  caption?: Child;
  /** What draws, placed with `at` in the drawing ({@link drawingOf} big). */
  children?: Child;
};

/**
 * What a device does to the sound, at the left of its card: two rows tall, four columns wide, or
 * six for a file or an EQ. Its scale is faint lines with faint words; a control sits in the strip
 * above the drawing; the caption at its foot says only what moves.
 */
export const Display = ({ columns = 4, strip, keepStrip = false, across = [], up = [], zero, caption, children }: DisplayProps) => {
  const stripped = strip !== undefined || keepStrip;
  const { w, h: tall } = drawingOf(columns, stripped);
  const line = (points: number[], color: "border" | "border-strong") => <shapes at={[0, 0]} w={w} h={tall} lines={[{ color, width: 1, points }]} />;
  return (
    <box w={w} h={DISPLAY_HEIGHT} radius="md" fill="well" clip>
      {strip !== undefined && <row at={[6, 6]} h={STRIP - 12} gap={1} align="center">{strip}</row>}
      <box at={[0, DISPLAY_HEIGHT - tall]} w={w} h={tall}>
        {(across.length > 0 || up.length > 0) && line([...across.flatMap(({ at }) => [at * w, 0, at * w, tall]), ...up.flatMap(({ at }) => [0, (1 - at) * tall, w, (1 - at) * tall])], "border")}
        {zero !== undefined && line([0, (1 - zero) * tall, w, (1 - zero) * tall], "border-strong")}
        {across.map(({ at, text }) => text !== undefined && <text at={[at * w + 4, 3]} style="label" color="text-muted">{text}</text>)}
        {up.map(({ at, text }) => text !== undefined && <text at={[4, (1 - at) * tall - 16]} style="label" color="text-muted">{text}</text>)}
        {children}
      </box>
      {caption !== undefined && <row at={[6, DISPLAY_HEIGHT - 22]} h={18} px={1} align="center" radius="sm" fill="well">{typeof caption === "string" ? <text style="number" color="text-secondary">{caption}</text> : caption}</row>}
    </box>
  );
};
