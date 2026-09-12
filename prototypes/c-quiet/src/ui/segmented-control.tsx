import type * as React from "react";
import { cn } from "@/lib/cn";

export interface SegmentedOption<T extends string> {
  value: T;
  label: React.ReactNode;
  /** Accessible name when `label` is an icon. */
  ariaLabel?: string;
}

export interface SegmentedControlProps<T extends string> {
  value: T;
  onValueChange: (value: T) => void;
  options: SegmentedOption<T>[];
  size?: "xs" | "sm" | "md";
  rounded?: boolean;
  /** Accessible name for the group. */
  label: string;
  className?: string;
}

const sizes = {
  xs: { wrap: "h-6 p-0.5 gap-px", item: "h-5 px-1.5 text-xs [&_svg]:size-3.5 min-w-5" },
  sm: { wrap: "h-7 p-0.5 gap-0.5", item: "h-6 px-2 text-xs [&_svg]:size-3.5 min-w-6" },
  md: { wrap: "h-8 p-1 gap-0.5", item: "h-6 px-2 text-sm [&_svg]:size-4 min-w-6" },
} as const;

/** Single-select group. Selected item gets a raised base surface. */
export function SegmentedControl<T extends string>({
  value,
  onValueChange,
  options,
  size = "sm",
  rounded = false,
  label,
  className,
}: SegmentedControlProps<T>) {
  const s = sizes[size];
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className={cn(
        "inline-flex items-center bg-alpha/5",
        rounded ? "rounded-full" : size === "xs" ? "rounded-md" : "rounded-lg",
        s.wrap,
        className,
      )}
    >
      {options.map((option) => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={selected}
            aria-label={option.ariaLabel}
            onClick={() => onValueChange(option.value)}
            className={cn(
              "focus-ring inline-flex items-center justify-center gap-1 whitespace-nowrap font-medium transition-colors duration-100",
              rounded ? "rounded-full" : size === "xs" ? "rounded-[4px]" : "rounded-md",
              s.item,
              selected
                ? "bg-gray-950/10 text-gray-950 shadow-[0_1px_0_0_rgba(0,0,0,0.25)]"
                : "text-gray-800 hover:bg-alpha/5 hover:text-gray-950",
            )}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
