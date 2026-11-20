import { h } from "../sdk";
import { corner, height, type Size } from "./control";

export type Option<T> = { value: T; text: string };

/** A choice of a few options side by side: one stop of Tab, the arrow keys choose. */
export const Segmented = <T extends string | number>({ id, label, options, value, size = "sm", onChange }: { id: string; label: string; options: readonly Option<T>[]; value: T; size?: Size; onChange: (value: T) => void }) => (
  <row key={id} label={label} role="radiogroup" group h={height(size)} pad={[1, 0]} align="center" radius={corner(size)} fill="fill">
    {options.map((option) => {
      const chosen = option.value === value;
      return (
        <row key={`${id}-${option.value}`} label={`${label} ${option.text}`} role="radio" value={chosen} h={height(size) - 6} pad={[2, 0]} align="center" justify="center" radius={size === "sm" ? "sm" : "md"} fill={chosen ? "selected" : undefined} hover="fill" onPress={() => onChange(option.value)}>
          <text style="label" color={chosen ? "text" : "text-secondary"}>{option.text}</text>
        </row>
      );
    })}
  </row>
);
