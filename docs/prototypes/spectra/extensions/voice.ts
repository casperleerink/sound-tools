import { adsr, floor, lowpass, mix, mtof, number, pattern, phasor, rise, saw, sine, tool, wrap } from "./sdk";

export const voice = tool({
  name: "voice",
  title: "Voice",
  when: "You want a slow melody of saw and bell that loops by itself",
  doc: "Eight notes of `notes` (MIDI pitches) round and round, `rate` notes a second, through a low-pass that opens with each note.",
  state: {
    notes: pattern({ length: 8, min: 36, max: 84, default: 60 }),
    rate: number({ min: 0.5, max: 8, default: 2, unit: "hz" }),
  },
  sound: ({ notes, rate }) => {
    const steps = phasor(rate.over(8)).times(8);
    const pitch = notes.at(floor(steps));
    const gate = wrap(steps).lt(0.6);
    const envelope = adsr(rise(gate).plus(gate), 5, 300, 0.3, 400);
    const tone = lowpass(saw(mtof(pitch)), envelope.times(3000).plus(200), 4);
    const bell = sine(mtof(pitch.plus(24))).times(envelope.times(envelope));
    return mix(tone, bell, 0.3).times(envelope).times(0.12);
  },
});
