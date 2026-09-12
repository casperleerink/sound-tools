import { cn } from "./cn";

/**
 * Text tabs in the Hooman style: inactive tabs at 40% opacity, active tab full.
 * Controlled. For pane tab strips see app/pane-tabs.
 */
export function Tabs<T extends string>({
  value,
  onValueChange,
  tabs,
  className,
  size = "sm",
}: {
  value: T;
  onValueChange: (v: T) => void;
  tabs: { value: T; label: React.ReactNode }[];
  className?: string;
  size?: "sm" | "md";
}) {
  return (
    <div role="tablist" className={cn("inline-flex items-center gap-4", className)}>
      {tabs.map((tab) => {
        const active = tab.value === value;
        return (
          <button
            key={tab.value}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onValueChange(tab.value)}
            className={cn(
              "relative inline-flex items-center font-medium transition-opacity duration-100",
              size === "sm" ? "h-7 text-sm" : "h-8 text-base",
              active ? "opacity-100" : "opacity-40 hover:opacity-100",
            )}
          >
            {tab.label}
            {active && <span className="absolute inset-x-0 -bottom-px h-px bg-gray-950" />}
          </button>
        );
      })}
    </div>
  );
}
