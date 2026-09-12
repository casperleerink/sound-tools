import * as React from "react";
import { cn } from "./cn";
import { clamp, snap } from "./use-drag-value";

export interface SliderProps {
  id?: string;
  value: number;
  onChange: (v: number) => void;
  min: number;
  max: number;
  step?: number;
  label: string;
  orientation?: "horizontal" | "vertical";
  /** Show the fill from `origin` instead of from min (e.g. 0 for bipolar). */
  origin?: number;
  /** Thickness of the track in px. Default 6 horizontal / 6 vertical. */
  thickness?: number;
  disabled?: boolean;
  className?: string;
  /** Tick positions as values. */
  ticks?: number[];
  /** Colour of the fill. Defaults to --accent. */
  color?: string;
}

/**
 * Absolute-position slider: click or drag anywhere on the track. Arrow keys nudge, shift = x10.
 */
export function Slider({
  id,
  value,
  onChange,
  min,
  max,
  step,
  label,
  orientation = "horizontal",
  origin,
  thickness = 6,
  disabled,
  className,
  ticks,
  color = "var(--accent)",
}: SliderProps) {
  const ref = React.useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = React.useState(false);
  const vertical = orientation === "vertical";

  const pct = (v: number) => ((v - min) / (max - min)) * 100;
  const fromPointer = (e: React.PointerEvent) => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const t = vertical ? 1 - (e.clientY - r.top) / r.height : (e.clientX - r.left) / r.width;
    const raw = min + clamp(t, 0, 1) * (max - min);
    onChange(clamp(snap(raw, step, min), min, max));
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const s = step ?? (max - min) / 100;
    const inc = e.shiftKey ? s * 10 : s;
    let next: number | null = null;
    if (e.key === "ArrowUp" || e.key === "ArrowRight") next = value + inc;
    if (e.key === "ArrowDown" || e.key === "ArrowLeft") next = value - inc;
    if (e.key === "Home") next = min;
    if (e.key === "End") next = max;
    if (next !== null) {
      e.preventDefault();
      onChange(clamp(snap(next, step ?? s, min), min, max));
    }
  };

  const from = pct(origin ?? min);
  const to = pct(value);
  const lo = Math.min(from, to);
  const hi = Math.max(from, to);

  return (
    <div
      ref={ref}
      id={id}
      role="slider"
      tabIndex={disabled ? -1 : 0}
      aria-label={label}
      aria-orientation={orientation}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      aria-disabled={disabled}
      onKeyDown={onKeyDown}
      onPointerDown={(e) => {
        if (disabled || e.button !== 0) return;
        (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
        setDragging(true);
        fromPointer(e);
      }}
      onPointerMove={(e) => dragging && fromPointer(e)}
      onPointerUp={() => setDragging(false)}
      onPointerCancel={() => setDragging(false)}
      className={cn(
        "group/slider relative flex touch-none select-none items-center rounded-md",
        vertical ? "h-full w-6 shrink-0 cursor-ns-resize justify-center" : "h-6 w-full min-w-0 cursor-ew-resize",
        disabled && "cursor-not-allowed opacity-40",
        className,
      )}
    >
      {/* track */}
      <div
        className={cn("relative rounded-full bg-alpha/10", vertical ? "h-full" : "w-full")}
        style={vertical ? { width: thickness } : { height: thickness }}
      >
        {/* fill */}
        <div
          className="absolute rounded-full"
          style={
            vertical
              ? { bottom: `${lo}%`, top: `${100 - hi}%`, left: 0, right: 0, background: color }
              : { left: `${lo}%`, right: `${100 - hi}%`, top: 0, bottom: 0, background: color }
          }
        />
        {ticks?.map((t) => (
          <span
            key={t}
            aria-hidden
            className="absolute bg-gray-950/25"
            style={
              vertical
                ? { bottom: `${pct(t)}%`, left: -4, right: -4, height: 1 }
                : { left: `${pct(t)}%`, top: -4, bottom: -4, width: 1 }
            }
          />
        ))}
        {/* thumb */}
        <div
          aria-hidden
          className={cn(
            "absolute rounded-full bg-gray-950 shadow-[0_1px_2px_rgba(0,0,0,0.5)] transition-transform duration-100",
            dragging ? "scale-110" : "group-hover/slider:scale-105",
          )}
          style={
            vertical
              ? { bottom: `calc(${to}% - 7px)`, left: "50%", marginLeft: -7, width: 14, height: 14 }
              : { left: `calc(${to}% - 7px)`, top: "50%", marginTop: -7, width: 14, height: 14 }
          }
        />
      </div>
    </div>
  );
}
