import { h, live, type Card } from "../../../sdk";
import { Display, drawingOf, FREQUENCY_TICKS, OCTAVES } from "../../display";
import { binder } from "../../field";
import { Knob } from "../../knob";
import { DeviceCard } from "../device-card";

/** The most bands, and how tall their columns stand. */
const [MOST, HEIGHT] = [64, 0.6];

/**
 * The vocoder: its bands side by side as soft columns over 20 Hz to 20 kHz, as many as it has;
 * then bands, release, formant and mix. Its voice is the rack's sidechain.
 */
export const card: Card<"vocoder"> = (self, ctx) => {
  const field = binder(self, ctx);
  const { w, h: tall } = drawingOf(4);
  const bands = live.field("bands").round();
  const across = w / OCTAVES[1];
  const columns = Array.from({ length: MOST }, (_, at) => {
    const [low, high] = [live.feed(`bands.${at}`), live.feed(`bands.${at + 1}`)];
    // A hairline apart.
    return <box at={[low.times(across).plus(0.5), (1 - HEIGHT) * tall]} w={high.minus(low).times(across).minus(1).max(0)} h={HEIGHT * tall} fill="text/16" opacity={bands.gt(at)} />;
  });
  return (
    <DeviceCard id="card" title={field.title} display={<Display across={FREQUENCY_TICKS}>{columns}</Display>}
      cells={[
        <Knob {...field.number("bands", "Bands")} step={1} />,
        <Knob {...field.number("release_ms", "Release")} />,
        <Knob {...field.number("formant", "Formant")} unit="semitones" bipolar />,
        <Knob {...field.number("mix", "Mix")} unit="percent" />,
      ]} />
  );
};
