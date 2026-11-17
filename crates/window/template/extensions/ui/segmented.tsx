import { h } from "../sdk";
import { corner, height, type Size } from "./control";

export type Option<T> = { value: T; text: string };

/** A choice of a few options side by side: one stop of Tab, the arrow keys choose. */
export const Segmented = <T extends string | number>({ id, label, options, value, size = "sm", onChange }: { id: string; label: string; options: readonly Option<T>[]; value: T; size?: Size; onChange: (value: T) => void }) => (
  <row key={id} label={label} role="radiogroup" group h={height(size)} px={1} align="center" radius={corner(size)} fill="fill">
    {options.map((option) => {
      const chosen = option.value === value;
      return (
        <row key={`${id}-${option.value}`} label={`${label} ${option.text}`} role="radio" checked={chosen} h={height(size) - 6} px={2} align="center" justify="center" radius={size === "sm" ? "sm" : "md"} fill={chosen ? "fill-strong" : undefined} hover="fill" onPress={() => onChange(option.value)}>
          <text style="control" color={chosen ? "text" : "text-secondary"}>{option.text}</text>
        </row>
      );
    })}
  </row>
);
