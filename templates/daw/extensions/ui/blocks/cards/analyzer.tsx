import { h, live, type Card, type Color, type Live } from "../../../sdk";
import theme from "../../../theme.json";
import { Cell } from "../../cell";
import { Display, drawingOf, FREQUENCY_TICKS } from "../../display";
import { Meter, Peak } from "../../meter";
import { DeviceCard } from "../device-card";

/** The levels of the spectrum, in dB: from the floor of the analysis up to full scale. */
const LEVELS: [number, number] = [0, -90];
/** Where a level turns yellow, then red, as on a meter. */
const [YELLOW_DB, RED_DB] = [-18, -6];
/** The room at the right for the meter, and around it. */
const [ROOM, INSET, METER] = [24, 6, 10];
const height = (db: number) => (db - LEVELS[1]) / (LEVELS[0] - LEVELS[1]);

/** A number the analyzer hears, in a cell, empty while it hears nothing: a note, or with `digits` and the words of its unit. */
const Heard = ({ id, label, text, span, value, shows, unit }: { id: string; label: string; text: string; span?: number; value: Live; shows: Live; unit?: string }) => (
  <Cell text={text} span={span}>
    <row key={id} label={label} value={value} h={24} gap={1} align="center" opacity={shows}>
      {unit === undefined ? <text style="number" color="text" live={value} unit="pitch" /> : [<text style="number" color="text" live={value} digits={1} />, <text style="number" color="text-secondary">{unit}</text>]}
    </row>
  </Cell>
);

/**
 * The analyzer: the spectrum of what it hears, green, yellow and red by height as on a meter,
 * and its level beside it; under the note it hears, its peak and its loudness. It has no
 * settings: this is all of it.
 */
export const card: Card<"analyzer"> = (self) => {
  const { title } = self.facts;
  const { w, h: tall } = drawingOf(4);
  const spectrum = w - ROOM;
  // The fill in a colour from its height up, each part of the drawing its own colour.
  const band = (from: number, to: number, color: Color) => {
    const [top, bottom] = [(1 - to) * tall, (1 - from) * tall];
    return (
      <box at={[0, top]} w={spectrum} h={bottom - top} clip>
        <shapes at={[0, -top]} w={spectrum} h={tall} units={[[0, 1], LEVELS]} lines={[{ color, width: 1.5, points: live.feed("spectrum"), fill: `${color}/45` as Color }]} />
      </box>
    );
  };
  const display = (
    <Display across={FREQUENCY_TICKS.map((tick) => ({ ...tick, at: (tick.at * spectrum) / w }))} up={[{ at: height(-30) }, { at: height(-60) }]}>
      {band(0, height(YELLOW_DB), "green")}
      {band(height(YELLOW_DB), height(RED_DB), "yellow")}
      {band(height(RED_DB), 1, "red")}
      <box at={[w - (ROOM + METER) / 2, INSET]}><Meter id="level" label={`${title} level`} path="" w={METER} h={tall - 2 * INSET} /></box>
    </Display>
  );
  const [note, loudness] = [live.feed("note"), live.feed("loudness")];
  const readings = (
    <col w={2 * theme.size.cell} gap={1}>
      <row>
        <Heard id="note" label={`${title} note`} text="Note" value={note} shows={note.gt(0)} />
        <Cell text="Peak"><Peak id="level" label={`${title} peak`} path="" w={theme.size.cell - 8} /></Cell>
      </row>
      <Heard id="loudness" label={`${title} loudness`} text="Loudness" span={2} value={loudness} shows={loudness.gt(LEVELS[1])} unit="LUFS" />
    </col>
  );
  return <DeviceCard id="card" title={title} display={display} cells={[readings]} />;
};
