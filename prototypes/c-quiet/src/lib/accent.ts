/** Musical tools pick one accent so instances can be told apart at a glance. */
export const accents = {
  blue: "var(--color-blue-500)",
  sapphire: "var(--color-blue-600)",
  sky: "var(--color-cyan-500)",
  teal: "var(--color-teal-500)",
  green: "var(--color-green-500)",
  yellow: "var(--color-yellow-500)",
  peach: "var(--color-orange-500)",
  red: "var(--color-red-500)",
  maroon: "var(--color-maroon-500)",
  mauve: "var(--color-purple-500)",
  pink: "var(--color-pink-500)",
  lavender: "var(--color-lavender-500)",
  rosewater: "var(--color-rosewater-500)",
  flamingo: "var(--color-flamingo-500)",
} as const;

export type Accent = keyof typeof accents;

/** CSS custom property block so children can use `var(--accent)`. */
export function accentStyle(accent: Accent): React.CSSProperties {
  return { "--accent": accents[accent] } as React.CSSProperties;
}
