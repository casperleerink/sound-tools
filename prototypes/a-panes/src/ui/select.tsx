import { ChevronDown } from "lucide-react";
import { cn } from "./cn";

/** Native select styled like a subtle button. Good enough for the prototype and fully keyboard-reachable. */
export function Select<T extends string>({
  id,
  value,
  onChange,
  options,
  label,
  size = "sm",
  className,
  disabled,
}: {
  id?: string;
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: string }[];
  label: string;
  size?: "xs" | "sm" | "md";
  className?: string;
  disabled?: boolean;
}) {
  const h = size === "xs" ? "h-6 pl-1.5 pr-6 text-xs" : size === "sm" ? "h-7 pl-2 pr-6 text-xs" : "h-8 pl-2.5 pr-7 text-sm";
  return (
    <span className={cn("relative inline-flex min-w-0", className)}>
      <select
        id={id}
        aria-label={label}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value as T)}
        className={cn(
          "w-full min-w-0 cursor-pointer appearance-none truncate rounded-md border border-alpha/10 bg-alpha/5 font-medium text-gray-950 transition-colors duration-100 hover:bg-alpha/10 disabled:cursor-not-allowed disabled:opacity-40",
          h,
        )}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value} className="bg-gray-300 text-gray-950">
            {o.label}
          </option>
        ))}
      </select>
      <ChevronDown
        aria-hidden
        className={cn("pointer-events-none absolute top-1/2 -translate-y-1/2 text-gray-950/50", size === "md" ? "right-2 size-4" : "right-1.5 size-3.5")}
      />
    </span>
  );
}
