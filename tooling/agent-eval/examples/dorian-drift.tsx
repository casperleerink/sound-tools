import { memo, abs, adsr, Controls, Display, feedback, floor, Fragment, h, Label, number, lowpass, phasor, pow, rise, sin, smooth, tool, TAU, output, wrap, type Operand, type Signal, Knob } from "./sdk";

// D dorian over two octaves: degree 0 is D3, degree 14 is D5.
const ROOT = 50;
const TOP = 14;
const DEGREES = ["D", "E", "F", "G", "A", "B", "C"];

// The same "random" number on both channels: noise() differs per channel, a hash of a counter does not.
const random = (count: Signal, seed: number): Signal =>
  wrap(sin(count.times(12.9898).plus(seed)).times(43758.5453));

const saw = (hz: Operand): Signal => phasor(hz).times(2).minus(1);

tool({
  name: "dorian-drift",
  title: "Dorian drift",
  when: "You want a pad that plays a slow generative ambient line in D dorian by itself",
  doc: [
    "A generative ambient voice. It needs no notes: it runs forever, transport playing or not, and wanders by steps of up to two scale degrees between the notes of D dorian from D3 to D5, turning back at the ends. Each note swells in slowly and fades; a few repeat or hold. Detuned saws through a soft low-pass. Put a long reverb after it for the wash.",
    "",
    "`density` is how often it moves to a new note: 0 is one every 12 s, 0.5 one every 4 s, 1 one every 1.5 s. The swells shorten with it, so notes overlap about as much at any setting.",
    "`brightness` opens the filter, from a dark 250 Hz at 0 through about 1.3 kHz at 0.5 to 7 kHz at 1, and adds more of the upper saw.",
    "`level` is the output gain; near 0.15 sits like the synth.",
  ].join("\n"),
  state: {
    density: number({ min: 0, max: 1, default: 0.35 }),
    brightness: number({ min: 0, max: 1, default: 0.4 }),
    level: number({ min: 0, max: 0.5, default: 0.15 }),
  },
  sound: ({ density, brightness, level }) => {
    const seconds = pow(0.125, density).times(12);
    const clock = phasor(pow(seconds, -1));
    const tick = rise(clock.lt(0.5));

    // How many notes have played: the seed of each choice.
    const count = feedback();
    count.set(count.plus(tick));

    // A walk of -2 to +2 degrees, reflected at the ends. Stored from the middle, so it starts on D4.
    const offset = feedback();
    const step = floor(random(count, 1.3).times(5)).minus(2);
    const moved = abs(offset.plus(7).plus(step));
    const next = abs(moved.minus(TOP)).negate().plus(TOP).minus(7);
    offset.set(offset.plus(tick.times(next.minus(offset))));

    const degree = offset.plus(7);
    const octave = floor(degree.over(7));
    const index = degree.minus(octave.times(7));
    const semitone = index.times(2).minus(index.ge(2)).minus(index.ge(6));
    const pitch = octave.times(12).plus(semitone).plus(ROOT);
    output("note", pitch);

    // A soft glide between notes, then a swell that takes most of the step.
    const hz = pow(2, smooth(pitch, 120).minus(69).over(12)).times(440);
    const ms = seconds.times(1000);
    const gate = clock.lt(0.55);
    const strength = random(count, 7.1).times(0.4).plus(0.6);
    const env = adsr(gate, ms.times(0.4), ms.times(0.3), 0.8, ms.times(0.9)).times(strength);

    const body = saw(hz.times(0.997)).plus(saw(hz.times(1.004))).times(0.5);
    const air = saw(hz.times(2.002)).times(brightness.times(0.35));
    const sub = sin(phasor(hz.times(0.5)).times(TAU)).times(0.4);
    const cutoff = pow(28, brightness).times(250);
    const tone = lowpass(lowpass(body.plus(air), cutoff), cutoff.times(1.5)).plus(sub);

    return tone.times(env).times(level);
  },
  view: ({ outputs }) => {
    const pitch = () => Math.round(outputs.note ?? 62);
    const name = memo(() => {
      const degree = (((pitch() - ROOT) % 12) + 12) % 12;
      return DEGREES[[0, 2, 3, 5, 7, 9, 10].indexOf(degree)] ?? "?";
    });
    const octave = () => Math.floor(pitch() / 12) - 1;
    return (
      <>
        <Display caption="D dorian">
          <Label x={0.5} y={0.15} text={() => `${name()}${octave()}`} variant="display" align="center" />
          <Label x={0.5} y={0.7} align="center"
            text={() => DEGREES.map((degree) => ({ text: ` ${degree} `, color: degree === name() ? "text" : "text-muted" }))} />
        </Display>
        <Controls>
          <Knob field="density" />
          <Knob field="brightness" />
          <Knob field="level" />
        </Controls>
      </>
    );
  },
});
