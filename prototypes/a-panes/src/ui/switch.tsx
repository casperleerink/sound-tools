import { cn } from "./cn";

export function Switch({
  checked,
  onCheckedChange,
  disabled,
  size = "sm",
  label,
  className,
}: {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  size?: "xs" | "sm";
  /** Accessible label. Visible labels belong in a LabelledControl. */
  label: string;
  className?: string;
}) {
  const track = size === "xs" ? "h-4 w-7" : "h-5 w-9";
  const thumb = size === "xs" ? "size-3" : "size-4";
  const travel = size === "xs" ? "translate-x-3" : "translate-x-4";
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onCheckedChange(!checked)}
      className={cn(
        "inline-flex shrink-0 items-center rounded-full p-0.5 transition-colors duration-100 disabled:cursor-not-allowed disabled:opacity-40",
        track,
        checked ? "bg-(--accent)" : "bg-alpha/10 hover:bg-alpha/15",
        className,
      )}
    >
      <span
        className={cn(
          "block rounded-full shadow-sm transition-transform duration-100",
          thumb,
          checked ? `${travel} bg-gray-200` : "translate-x-0 bg-gray-950/80",
        )}
      />
    </button>
  );
}
