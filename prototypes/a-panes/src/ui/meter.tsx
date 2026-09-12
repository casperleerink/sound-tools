import { cn } from "./cn";

export interface MeterProps {
  /** Level in dBFS, from -60 to +6. */
  level: number;
  /** Peak hold in dBFS. */
  peak?: number;
  orientation?: "horizontal" | "vertical";
  /** Track thickness in px. */
  thickness?: number;
  label: string;
  className?: string;
  /** Show -12/-6/0 tick marks. */
  scale?: boolean;
}

const MIN = -60;
const MAX = 6;
const toPct = (db: number) => ((Math.min(MAX, Math.max(MIN, db)) - MIN) / (MAX - MIN)) * 100;

/** Level meter with a soft green → yellow → red gradient and a peak line. */
export function Meter({
  level,
  peak,
  orientation = "vertical",
  thickness = 6,
  label,
  className,
  scale,
}: MeterProps) {
  const vertical = orientation === "vertical";
  const pct = toPct(level);
  const gradient = vertical
    ? "linear-gradient(to top, var(--color-green-500) 0%, var(--color-green-500) 72%, var(--color-yellow-500) 88%, var(--color-red-500) 100%)"
    : "linear-gradient(to right, var(--color-green-500) 0%, var(--color-green-500) 72%, var(--color-yellow-500) 88%, var(--color-red-500) 100%)";
  return (
    <div
      role="meter"
      aria-label={label}
      aria-valuemin={MIN}
      aria-valuemax={MAX}
      aria-valuenow={Math.round(level)}
      aria-valuetext={`${level.toFixed(1)} dB`}
      className={cn("relative overflow-hidden rounded-[3px] bg-alpha/8", vertical ? "h-full shrink-0" : "w-full min-w-0", className)}
      style={vertical ? { width: thickness } : { height: thickness }}
    >
      <div
        className="absolute inset-0"
        style={{
          background: gradient,
          clipPath: vertical ? `inset(${100 - pct}% 0 0 0)` : `inset(0 ${100 - pct}% 0 0)`,
        }}
      />
      {peak !== undefined && (
        <div
          className={cn("absolute", peak >= 0 ? "bg-red-500" : "bg-gray-950/80")}
          style={
            vertical
              ? { bottom: `${toPct(peak)}%`, left: 0, right: 0, height: 1 }
              : { left: `${toPct(peak)}%`, top: 0, bottom: 0, width: 1 }
          }
        />
      )}
      {scale &&
        [-12, -6, 0].map((db) => (
          <span
            key={db}
            aria-hidden
            className="absolute bg-gray-50/60"
            style={vertical ? { bottom: `${toPct(db)}%`, left: 0, right: 0, height: 1 } : { left: `${toPct(db)}%`, top: 0, bottom: 0, width: 1 }}
          />
        ))}
    </div>
  );
}
