import { cn } from "./cn";

export interface SegmentedOption<T extends string> {
  value: T;
  label: React.ReactNode;
  ariaLabel?: string;
}

export function SegmentedControl<T extends string>({
  value,
  onValueChange,
  options,
  size = "sm",
  className,
  "aria-label": ariaLabel,
}: {
  value: T;
  onValueChange: (value: T) => void;
  options: SegmentedOption<T>[];
  size?: "xs" | "sm" | "md";
  className?: string;
  "aria-label"?: string;
}) {
  const h = size === "xs" ? "h-5 px-1.5 text-xs" : size === "sm" ? "h-6 px-2 text-xs" : "h-7 px-2.5 text-sm";
  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      className={cn("inline-flex items-center gap-0.5 rounded-lg bg-alpha/5 p-0.5", className)}
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
              "inline-flex items-center justify-center gap-1 rounded-md font-medium transition-colors duration-100 [&_svg]:size-3.5",
              h,
              selected
                ? "bg-gray-300 text-gray-950 shadow-[0_1px_0_rgba(0,0,0,0.25)]"
                : "text-gray-950/60 hover:bg-alpha/5 hover:text-gray-950",
            )}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
