import {
  adsr,
  beat,
  clamp,
  floor,
  h,
  Knob,
  knob,
  lowpass,
  pattern,
  phasor,
  playing,
  pow,
  saturate,
  smooth,
  Steps,
  tool,
  watch,
  wrap,
} from "./sdk";

tool({
  name: "acid-bass",
  title: "Acid bass",
  when: "A track plays a 16-step acid bass line that follows the tempo of the piece",
  doc: [
    "A 303-style acid bass with its own 16-step sequencer. It plays by itself in time with the piece, one step per sixteenth note, the pattern repeating every bar; it needs no clips and ignores the notes of its track.",
    "",
    "Each step is a saw note through a resonant low-pass filter that opens on the hit and closes again within about a fifth of a second: the squelch.",
    "",
    "- `steps`: which of the 16 sixteenths play, 1 on, 0 off. Two steps on in a row are two hits.",
    "- `accents`: steps that hit harder, louder and with the filter opening further. An accent on a step that is off does nothing.",
    "- `cutoff`: where the filter sits between hits. Low (100 to 300 Hz) is a dark, rubbery thump; around 600 Hz it bites; high is bright and buzzy. The hit opens it to about five times this.",
    "- `resonance`: the peak at the cutoff. 0 is a plain bass, 0.5 squelchy, 1 screaming acid.",
    "- `note`: the pitch every step plays, as a MIDI number in whole semitones: 36 is C2, 48 C3.",
  ].join("\n"),
  kind: "source",
  state: {
    steps: pattern({ length: 16, min: 0, max: 1, default: 0, label: "Steps" }),
    accents: pattern({ length: 16, min: 0, max: 1, default: 0, label: "Accents" }),
    cutoff: knob({ min: 60, max: 4000, default: 400, unit: "hz", label: "Cutoff" }),
    resonance: knob({ min: 0, max: 1, default: 0.6, label: "Resonance" }),
    note: knob({ min: 24, max: 60, default: 36, label: "Note" }),
  },
  sound: ({ steps, accents, cutoff, resonance, note }) => {
    const sixteenths = beat.times(4);
    const step = floor(sixteenths).mod(16);
    watch("step", step);

    const on = steps.at(step).gt(0.5);
    const accent = on.times(accents.at(step).gt(0.5));
    // Held for the first half of each step, so two steps in a row hit twice.
    const gate = on.times(wrap(sixteenths).lt(0.5)).times(playing);

    const pitch = floor(note.plus(0.5));
    const freq = pow(2, pitch.minus(69).over(12)).times(440);
    const saw = phasor(freq).times(2).minus(1);

    const env = adsr(gate, 1, 180, 0, 40);
    const amp = adsr(gate, 2, 400, 0.55, 30);
    const level = smooth(accent, 5).times(0.6).plus(1);
    const opening = env.times(level).times(4).plus(1);
    const hz = clamp(cutoff.times(opening), 30, 16000);
    const q = resonance.times(15).plus(0.7);

    const filtered = lowpass(lowpass(saw, hz), hz, q);
    return saturate(filtered.times(amp).times(level).times(0.9)).times(0.13);
  },
  card: () => (
    <div style={{ gap: 8 }}>
      <div style={{ direction: "row", gap: 8, align: "center" }}>
        <Knob path="cutoff" label="Cutoff" min={60} max={4000} default={400} unit="hz" />
        <Knob path="resonance" label="Resonance" min={0} max={1} default={0.6} />
        <Knob path="note" label="Note" min={24} max={60} default={36} />
      </div>
      <div style={{ color: "#9ca3af" }}>Steps</div>
      <Steps path="steps" playing="step" />
      <div style={{ color: "#9ca3af" }}>Accents</div>
      <Steps path="accents" playing="step" />
    </div>
  ),
});
