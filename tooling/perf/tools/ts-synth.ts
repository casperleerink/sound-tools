import { adsr, knob, lowpass, note, saw, tool } from "./sdk";

// The built-in synth (instrument.synth) rebuilt as a tool, with its defaults: a saw through a
// low pass, shaped by an ADSR. The performance harness plays both on the same notes.
tool({
  name: "ts-synth",
  title: "TS synth",
  when: "The performance harness compares it with the built-in synth",
  doc: "A saw through a low-pass filter, shaped by an ADSR envelope. Loudness follows velocity squared.",
  kind: "instrument",
  state: {
    cutoff: knob({ min: 20, max: 20000, default: 2000, unit: "hz", label: "Cutoff" }),
    q: knob({ min: 0.5, max: 16, default: 1.3, label: "Resonance" }),
    attack: knob({ min: 1, max: 10000, default: 5, unit: "ms", label: "Attack" }),
    decay: knob({ min: 1, max: 10000, default: 200, unit: "ms", label: "Decay" }),
    sustain: knob({ min: 0, max: 1, default: 0.7, label: "Sustain" }),
    release: knob({ min: 1, max: 10000, default: 300, unit: "ms", label: "Release" }),
    gain: knob({ min: 0, max: 1, default: 0.15, label: "Gain" }),
  },
  sound: ({ cutoff, q, attack, decay, sustain, release, gain }) => {
    const level = adsr(note.gate, attack, decay, sustain, release);
    const loudness = note.velocity.times(note.velocity);
    return lowpass(saw(note.freq), cutoff, q).times(level).times(loudness).times(gain);
  },
});
