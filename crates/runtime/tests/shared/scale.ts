import { choice, editor, toggle } from "./sdk";

const STEPS = {
  major: [0, 2, 4, 5, 7, 9, 11],
  minor: [0, 2, 3, 5, 7, 8, 10],
  dorian: [0, 2, 3, 5, 7, 9, 10],
};
const ROOTS = ["C", "D", "E", "F", "G", "A", "B"] as const;
const SEMITONES = [0, 2, 4, 5, 7, 9, 11];
const LOWEST = 48;
const HIGHEST = 84;

editor({
  name: "scale",
  title: "Scale",
  clip: {
    scale: choice({ options: ["major", "minor", "dorian"], default: "major" }),
    root: choice({ options: ROOTS, default: "C" }),
  },
  note: { accent: toggle({ default: false, label: "Accent" }) },
  grid: ({ scale, root }) => {
    const offset = SEMITONES[ROOTS.indexOf(root)];
    const rows = [];
    for (let pitch = LOWEST; pitch <= HIGHEST; pitch++) {
      const step = (pitch - offset + 12) % 12;
      if (STEPS[scale].includes(step)) {
        rows.push(step === 0 ? { pitch, label: `${root}${Math.floor(pitch / 12) - 1}` } : { pitch });
      }
    }
    return { rows, rowHeight: 16 };
  },
});
