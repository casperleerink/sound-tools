import { h } from "../sdk";
import { Segmented, type Option } from "./segmented";
import { Select } from "./select";
import type { Size } from "./control";

/** The most options, and their letters, side by side; more are a list. */
const [SIDE_BY_SIDE, LETTERS] = [4, 20];

export type ChoiceProps<T extends string | number> = { id: string; label: string; options: readonly Option<T>[]; value: T; onChange: (value: T) => void; size?: Size; w?: number };

/** One of some options, bound as `field.choice("type")` gives it: side by side when they are few and short, else a list. */
export const Choice = <T extends string | number>({ id, label, options, value, onChange, size = "sm", w = 120 }: ChoiceProps<T>) =>
  options.length <= SIDE_BY_SIDE && options.reduce((sum, option) => sum + option.text.length, 0) <= LETTERS
    ? <Segmented id={id} label={label} options={options} value={value} size={size} onChange={onChange} />
    : <Select id={id} label={label} options={options} value={value} size={size} w={w} search={options.length > 8} onChange={onChange} />;
