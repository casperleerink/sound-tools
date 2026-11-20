import { h } from "../sdk";
import { corner, height, type Size } from "./control";
import type { Option } from "./segmented";

/** One option of many, as a button that opens the list of them. */
export const Select = <T extends string | number>({ id, label, options, value, size = "sm", w = 120, onChange }: { id: string; label: string; options: readonly Option<T>[]; value: T; size?: Size; w?: number; onChange: (value: T) => void }) => (
  <row key={id} label={label} role="button" value={options.find((option) => option.value === value)?.text ?? ""} w={w} h={height(size)} px={2} gap={1} align="center" justify="between" radius={corner(size)} fill="fill" hover="fill">
    <text style="label" color="text" truncate>{options.find((option) => option.value === value)?.text ?? ""}</text>
    <icon name="chevron-down" size={14} color="text-muted" />
    <col overlay={{ open: "press", side: "bottom", align: "start", gap: 4 }} role="listbox" group label={label} w={w} pad={1} radius="lg" fill="raised" border="border-strong" shadow="md">
      {options.map((option) => {
        const chosen = option.value === value;
        return (
          <row key={`${id}-${option.value}`} label={`${label} ${option.text}`} role="option" checked={chosen} w="full" h={height("sm")} px={2} align="center" radius="md" fill={chosen ? "selected" : undefined} hover="fill" onPress={() => onChange(option.value)}>
            <text style="label" color={chosen ? "text" : "text-secondary"}>{option.text}</text>
          </row>
        );
      })}
    </col>
  </row>
);
