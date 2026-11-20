import { h, type IconName } from "../sdk";
import { corner, height, type Size } from "./control";
import { Search } from "./menu";
import type { Option } from "./segmented";

/** One option of many, as a button that opens the list of them; `search` adds a line that keeps the options it names. An option's `detail` shows under it, its `icon` before it; with every option's `icon`, `iconOnly` shows the chosen one's icon alone, for a narrow cell that says its name. */
export const Select = <T extends string | number>({ id, label, options, value, size = "sm", w = 120, menuW = w, search = false, iconOnly = false, onChange }: { id: string; label: string; options: readonly (Option<T> & { detail?: string; icon?: IconName })[]; value: T; size?: Size; w?: number; menuW?: number; search?: boolean; iconOnly?: boolean; onChange: (value: T) => void }) => (
  <row key={id} label={label} role="button" value={options.find((option) => option.value === value)?.text ?? ""} w={w} h={height(size)} pad={[2, 0]} gap={1} align="center" justify="between" radius={corner(size)} fill="fill" hover="fill">
    {iconOnly && options.find((option) => option.value === value)?.icon !== undefined
      ? <icon name={options.find((option) => option.value === value)!.icon!} size={16} color="text" />
      : <text style="label" color="text" truncate>{options.find((option) => option.value === value)?.text ?? ""}</text>}
    <icon name="chevron-down" size={14} color="text-muted" />
    <col overlay={{ open: "press", side: "bottom", align: "start", gap: 4 }} role="listbox" group label={label} w={menuW} maxH={320} scroll="y" pad={1} radius="lg" fill="raised" border="border-strong" shadow="md">
      {search && <Search id={id} label={label} w={menuW} />}
      {options.map((option) => {
        const chosen = option.value === value;
        return (
          <col key={`${id}-${option.value}`} label={option.text} role="option" value={chosen} w="full" {...(option.detail === undefined ? { h: height("sm") } : {})} pad={[2, option.detail === undefined ? 0 : 1]} justify="center" radius="md" fill={chosen ? "selected" : undefined} hover="fill" onPress={() => onChange(option.value)}>
            <row gap={2} align="center">
              {option.icon !== undefined && <icon name={option.icon} size={16} color={chosen ? "text" : "text-secondary"} />}
              <text style="label" color={chosen ? "text" : "text-secondary"}>{option.text}</text>
            </row>
            {option.detail !== undefined && <text style="label" color="text-muted">{option.detail}</text>}
          </col>
        );
      })}
    </col>
  </row>
);
