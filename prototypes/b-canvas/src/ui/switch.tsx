import * as React from "react";
import { cn } from "@/lib/cn";

export interface SwitchProps {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  size?: "xs" | "sm";
  disabled?: boolean;
  /** Accessible name. */
  label: string;
  /** Use the instance accent instead of the neutral text colour when on. */
  accent?: boolean;
  className?: string;
  [dataAttr: `data-${string}`]: string | undefined;
}

const track = { xs: "h-4 w-7 p-0.5", sm: "h-5 w-9 p-0.5" };
const thumb = { xs: "size-3 data-checked:translate-x-3", sm: "size-4 data-checked:translate-x-4" };

export const Switch = React.forwardRef<HTMLButtonElement, SwitchProps>(
  ({ checked, onCheckedChange, size = "sm", disabled, label, accent, className, ...rest }, ref) => (
    <button
      ref={ref}
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onCheckedChange(!checked)}
      className={cn(
        "focus-ring inline-flex shrink-0 items-center rounded-full transition-colors duration-100 disabled:cursor-not-allowed disabled:opacity-40",
        track[size],
        checked ? (accent ? "bg-(--accent)" : "bg-gray-950") : "bg-alpha/10",
        className,
      )}
      {...rest}
    >
      <span
        data-checked={checked ? "" : undefined}
        className={cn(
          "pointer-events-none block rounded-full shadow-[0_1px_2px_rgba(0,0,0,0.4)] transition-transform duration-100 ease-fluid",
          checked ? "bg-ink" : "bg-gray-900",
          thumb[size],
        )}
      />
    </button>
  ),
);
Switch.displayName = "Switch";
