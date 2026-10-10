import {
  bandpass,
  clamp,
  db,
  exp,
  feedback,
  h,
  highpass,
  hold,
  Knob,
  knob,
  live,
  lowpass,
  max,
  Meter,
  mix,
  noise,
  Pad,
  phasor,
  pow,
  rise,
  sampleRate,
  saturate,
  type Signal,
  smooth,
  sqrt,
  type Style,
  tool,
  trigger,
  watch,
} from "./sdk";

/** A slow random wander from -1 to 1: a new target `hz` times a second, glided over `ms`. */
const gust = (hz: Signal | number, ms: Signal | number): Signal =>
  smooth(hold(noise(), rise(phasor(hz).minus(0.5))), ms);

const row: Style = { direction: "row", gap: 8, align: "center" };
const button: Style = { paddingX: 12, paddingY: 8, radius: 6, background: "#3a3350" };

tool({
  name: "storm",
  title: "Storm",
  kind: "source",
  when: "You want wind, a stormy noise drone, or thunder that plays by itself with no notes",
  doc: [
    "A storm that blows all the time, notes or not: wind made of filtered noise that gusts and howls by itself, with thunder on a button.",
    "",
    "It is played live from its card. The pad's across, `tone`, goes from dark (a low, muffled roar, 0) to bright (a thin, hissing whistle, 1); the middle is an open, full wind. The pad's up, `wild`, goes from a calm, slow breeze (0) to a howling gale (1): faster, deeper gusts, a ringing whistle and gritty spray. Halfway is a steady, moving wind. **Thunder** sets off a deep rumble that rolls and dies away over `thunder_decay`.",
    "",
    "`level` is the wind's loudness. `thunder` is how loud a thunderclap is next to the wind: 0 turns thunder off, 1 is a big, close clap. `thunder_decay` is how long the rumble rolls on.",
  ].join("\n"),
  state: {
    level: knob({ min: -30, max: 6, default: -10, unit: "db", label: "Level" }),
    thunder: knob({ min: 0, max: 1, default: 0.7, unit: "percent", label: "Thunder" }),
    thunder_decay: knob({ min: 500, max: 8000, default: 3000, unit: "ms", label: "Rumble" }),
  },
  controls: {
    tone: live({ min: 0, max: 1, default: 0.4, label: "Dark / bright" }),
    wild: live({ min: 0, max: 1, default: 0.3, label: "Wild" }),
    strike: trigger({ label: "Thunder" }),
  },
  sound: ({ level, thunder, thunder_decay, tone, wild, strike }) => {
    // Wind: noise through a band that wanders with the gusts.
    const rate = mix(0.15, 2.5, wild);
    const sway = gust(rate, mix(1500, 250, wild));
    const center = mix(180, 7000, pow(tone, 1.6)).times(pow(2, sway.times(mix(0.6, 2.2, wild))));
    const hz = clamp(center, 60, 16000);
    const q = mix(0.8, 6, wild);
    // Keep the loudness steady as the band narrows or moves.
    const norm = clamp(sqrt(q.times(24000).over(hz)).times(0.25), 0, 8);
    const body = bandpass(noise(), hz, q).times(norm);

    // A second, slower band an octave and a bit up, for a fuller howl.
    const sway2 = gust(rate.times(0.7), mix(2000, 400, wild));
    const hz2 = clamp(center.times(1.7).times(pow(2, sway2.times(0.8))), 80, 16000);
    const body2 = bandpass(noise(), hz2, q.times(1.5)).times(
      clamp(sqrt(q.times(36000).over(hz2)).times(0.12), 0, 8),
    );

    // A low roar under it all, louder when dark, and hiss on top, louder when bright and wild.
    const roar = lowpass(noise(), mix(90, 250, tone)).times(mix(4, 1.5, tone));
    const hiss = highpass(noise(), 4000).times(tone.times(mix(0.03, 0.1, wild)));

    // Gusts swell the whole wind; wild makes them deeper.
    const swell = gust(rate.times(0.5), mix(2000, 500, wild)).times(mix(0.25, 0.6, wild)).plus(1);
    const wind = saturate(
      body.plus(body2).plus(roar).plus(hiss).times(swell).times(mix(1, 1.15, wild)),
    ).times(db(level));

    // Thunder: an envelope that jumps to 1 on the button and dies away over thunder_decay.
    const env = feedback();
    // Per sample, so that it falls by 60 dB over thunder_decay.
    const fall = exp(pow(sampleRate.times(thunder_decay), -1).times(-6908));
    env.set(max(strike, env.times(fall)));
    const swellUp = smooth(env, 120);
    const roll = gust(4, 120).times(0.45).plus(0.75);
    // It darkens as it fades, and its loudness falls half as fast as the envelope.
    const cut = swellUp.times(300).plus(70);
    const rumble = saturate(
      lowpass(noise(), cut, 1.2)
        .times(sqrt(pow(cut, -1).times(24000)))
        .times(sqrt(swellUp))
        .times(roll)
        .times(1.2),
    );
    const crack = highpass(noise(), 500)
      .times(pow(smooth(env, 10), 12))
      .times(1.2);
    watch("rumble", swellUp);

    return wind.plus(rumble.plus(crack).times(thunder));
  },
  card: ({ fire }) => (
    <div style={row}>
      <Pad x="tone" y="wild" size={160} />
      <div style={{ gap: 8 }}>
        <div style={button} onClick={() => fire("strike")}>
          Thunder
        </div>
        <Meter watch="rumble" label="Rumble" />
        <Knob path="level" label="Level" min={-30} max={6} default={-10} unit="db" />
        <Knob path="thunder" label="Thunder" min={0} max={1} default={0.7} unit="percent" />
        <Knob path="thunder_decay" label="Rumble" min={500} max={8000} default={3000} unit="ms" />
      </div>
    </div>
  ),
});
