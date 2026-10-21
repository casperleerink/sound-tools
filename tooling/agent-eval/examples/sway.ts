import { number, output, phasor, sin, TAU, tool } from "./sdk";

export const sway = tool({
  name: "sway",
  title: "Sway",
  when: "You want a number that swings slowly from -0.8 to 0.8, to move a knob of another child",
  doc: "Sends `pan`, a sine `rate` times a second, for a parent to connect to a number of a child, such as the pan of a utility.",
  state: { rate: number({ min: 0.05, max: 4, default: 0.25, unit: "hz" }) },
  sound: ({ rate }) => {
    output("pan", sin(phasor(rate).times(TAU)).times(0.8));
    return 0;
  },
});
