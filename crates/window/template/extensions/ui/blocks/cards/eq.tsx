import { h, live, type Card, type IconName } from "../../../sdk";
import theme from "../../../theme.json";
import { Cell } from "../../cell";
import { Curve } from "../../curve";
import { Display, drawingOf, FREQUENCY_TICKS, OCTAVES } from "../../display";
import { cardBinder } from "../../field";
import { Handle } from "../../handle";
import { Knob } from "../../knob";
import { Segmented } from "../../segmented";
import { Select } from "../../select";
import { Toggle } from "../../toggle";
import { shown } from "../../value";
import { DeviceCard } from "../device-card";

/** The ±15 dB of a band with room for its handle. */
const DB: [number, number] = [18, -18];
const GAIN_TRAVEL = { min: DB[1], max: DB[0] };
const BANDS = [0, 1, 2, 3] as const;
type Band = (typeof BANDS)[number];
/** A cut or a notch takes sound away and has no gain: its handle stays on the 0 dB line. */
const hasGain = (shape: string | null | undefined) => shape !== "low_cut" && shape !== "notch" && shape !== "high_cut";

/**
 * The EQ: the summed response with a numbered handle per band (frequency across, gain up), the
 * band the knobs show above it, then that band's frequency, gain, Q and shape; behind More each
 * band's switch and the output.
 */
export const card: Card<"eq"> = (self, ctx) => {
  const field = cardBinder<"eq">(self, ctx);
  const { memory } = ctx;
  const { w, h: tall } = drawingOf(6, true);
  const bands = BANDS.filter((band) => band < self.state.bands.length);
  const chosen: Band = bands.find((band) => band === memory.band) ?? 0;
  const choose = (band: Band) => (memory.band = band);
  const path = <K extends "frequency_hz" | "gain_db" | "q">(band: Band, name: K) => `bands[${band}].${name}` as const;
  const handle = (band: Band) => {
    const { shape, on } = self.state.bands[band]!;
    const [frequency, gain] = [field.number(path(band, "frequency_hz"), "Frequency"), field.number(path(band, "gain_db"), "Gain")];
    const gains = hasGain(shape);
    return (
      <Handle id={`point-${band + 1}`} label={`${field.title} point ${band + 1}`} w={w} h={tall} number={String(band + 1)} chosen={band === chosen} dimmed={!on}
        x={frequency} y={gains ? { ...gain, travel: GAIN_TRAVEL } : 0.5}
        caption={band === chosen ? undefined : gains ? `${shown(frequency.value, "hz")} · ${shown(gain.value, "db")}` : shown(frequency.value, "hz")}
        onPress={() => choose(band)}
        onChange={(hz, db) => {
          choose(band);
          field.change(gains ? "Change frequency and gain" : "Change frequency", { [path(band, "frequency_hz")]: hz, ...(gains ? { [path(band, "gain_db")]: db } : {}) });
        }} />
    );
  };
  const strip = <Segmented id="band" label={`${field.title} band shown`} options={bands.map((band) => ({ value: band, text: String(band + 1) }))} value={chosen} onChange={choose} />;
  const display = (
    <Display columns={6} strip={strip} across={FREQUENCY_TICKS} zero={0.5}>
      <Curve heights={live.feed("response")} w={w} h={tall} across={OCTAVES} range={DB} />
      {/* The chosen band last, over any other at its place. */}
      {[...bands.filter((band) => band !== chosen), chosen].map(handle)}
    </Display>
  );
  const shape = field.choice(`bands[${chosen}].shape`, "Shape");
  const gain = <Knob {...field.number(path(chosen, "gain_db"), "Gain")} bipolar />;
  return (
    <DeviceCard id="card" title={field.title} display={display}
      cells={[
        <Knob {...field.number(path(chosen, "frequency_hz"), "Freq")} />,
        hasGain(self.state.bands[chosen]!.shape) ? gain : <box w={theme.size.cell} opacity={0.4}>{gain}</box>,
        <Knob {...field.number(path(chosen, "q"), "Q")} />,
        <Cell text={`Band ${chosen + 1}`} value={shape.options.find((option) => option.value === shape.value)?.text}>
          <Select {...shape} options={shape.options.map((option) => ({ ...option, icon: `eq-${String(option.value).replace("_", "-")}` as IconName }))} w={theme.size.cell - 8} menuW={160} iconOnly />
        </Cell>,
      ]}
      more={[
        <Cell text="Bands" span={2}><row gap={1}>{bands.map((band) => <Toggle {...field.toggle(`bands[${band}].on`, `Band ${band + 1}`)} text={String(band + 1)} />)}</row></Cell>,
        <Knob {...field.number("output_gain_db", "Output")} bipolar />,
      ]}
      expanded={memory.expanded === true} onExpand={() => (memory.expanded = memory.expanded !== true)} />
  );
};
