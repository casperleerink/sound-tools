import { cn } from "@/lib/cn";

export interface MeterProps {
  /** Levels in dBFS, one per channel. */
  levels: number[];
  /** Peak hold per channel, in dBFS. */
  peaks?: number[];
  min?: number;
  orientation?: "horizontal" | "vertical";
  size?: "sm" | "md";
  label?: string;
  className?: string;
}

const toPct = (db: number, min: number) => Math.min(100, Math.max(0, ((db - min) / -min) * 100));

/** Level meter. Green up to -12, yellow up to -3, red above. Static in the prototype. */
export function Meter({ levels, peaks, min = -60, orientation = "vertical", size = "md", label, className }: MeterProps) {
  const vertical = orientation === "vertical";
  const thick = size === "sm" ? 4 : 6;
  return (
    <div
      role="meter"
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={0}
      aria-valuenow={Math.max(...levels)}
      className={cn("flex gap-0.5", vertical ? "h-full flex-row" : "w-full flex-col", className)}
    >
      {levels.map((level, i) => {
        const pct = toPct(level, min);
        const peak = peaks?.[i];
        return (
          <div
            key={i}
            className="relative overflow-hidden rounded-[2px] bg-alpha/10"
            style={vertical ? { width: thick, height: "100%" } : { height: thick, width: "100%" }}
          >
            <div
              className="absolute"
              style={{
                ...(vertical ? { left: 0, right: 0, bottom: 0, height: `${pct}%` } : { top: 0, bottom: 0, left: 0, width: `${pct}%` }),
                background: vertical
                  ? `linear-gradient(to top, var(--color-green-500) 0%, var(--color-green-500) ${toPct(-12, min) / (pct / 100)}%, var(--color-yellow-500) ${toPct(-3, min) / (pct / 100)}%, var(--color-red-500) 100%)`
                  : `linear-gradient(to right, var(--color-green-500) 0%, var(--color-green-500) ${toPct(-12, min) / (pct / 100)}%, var(--color-yellow-500) ${toPct(-3, min) / (pct / 100)}%, var(--color-red-500) 100%)`,
              }}
            />
            {peak !== undefined ? (
              <div
                className={cn("absolute", peak > -3 ? "bg-red-500" : "bg-gray-950")}
                style={
                  vertical
                    ? { left: 0, right: 0, height: 1, bottom: `${toPct(peak, min)}%` }
                    : { top: 0, bottom: 0, width: 1, left: `${toPct(peak, min)}%` }
                }
              />
            ) : null}
          </div>
        );
      })}
    </div>
  );
}
