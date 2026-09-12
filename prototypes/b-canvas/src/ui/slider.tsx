import * as React from "react";
import { cn } from "@/lib/cn";

export interface SliderProps {
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  orientation?: "horizontal" | "vertical";
  /** Track thickness. */
  size?: "sm" | "md";
  /** Fill the track from the left/bottom in the accent colour. Default true. */
  fill?: boolean;
  /** Draw the fill from a centre point (for bipolar values like pan). */
  bipolar?: boolean;
  /** Ghost marker showing the modulated (effective) value. */
  modulated?: number;
  label?: string;
  id?: string;
  "aria-labelledby"?: string;
  "aria-valuetext"?: string;
  disabled?: boolean;
  className?: string;
}

/** Linear slider. Pointer drag, arrow keys, Home/End. Uses `--accent` for the fill. */
export function Slider({
  value,
  onChange,
  min = 0,
  max = 1,
  step = 0,
  orientation = "horizontal",
  size = "md",
  fill = true,
  bipolar = false,
  modulated,
  label,
  id,
  disabled,
  className,
  ...rest
}: SliderProps) {
  const ref = React.useRef<HTMLDivElement>(null);
  const vertical = orientation === "vertical";
  const pct = ((value - min) / (max - min)) * 100;
  const modPct = modulated === undefined ? null : ((modulated - min) / (max - min)) * 100;
  const centre = bipolar ? ((0 - min) / (max - min)) * 100 : 0;

  const clamp = (v: number) => {
    let n = Math.min(max, Math.max(min, v));
    if (step > 0) n = Math.round((n - min) / step) * step + min;
    return n;
  };

  const fromPointer = (e: React.PointerEvent) => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const t = vertical ? 1 - (e.clientY - r.top) / r.height : (e.clientX - r.left) / r.width;
    onChange(clamp(min + Math.min(1, Math.max(0, t)) * (max - min)));
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const s = step > 0 ? step : (max - min) / 100;
    const big = s * 10;
    const map: Record<string, number | undefined> = {
      ArrowRight: s,
      ArrowUp: s,
      ArrowLeft: -s,
      ArrowDown: -s,
      PageUp: big,
      PageDown: -big,
    };
    if (e.key === "Home") onChange(min);
    else if (e.key === "End") onChange(max);
    else if (map[e.key] !== undefined) onChange(clamp(value + (e.shiftKey ? map[e.key]! * 10 : map[e.key]!)));
    else return;
    e.preventDefault();
  };

  const thick = size === "sm" ? "4px" : "6px";
  const fillStart = bipolar ? Math.min(centre, pct) : 0;
  const fillEnd = bipolar ? Math.max(centre, pct) : pct;

  return (
    <div
      ref={ref}
      role="slider"
      id={id}
      aria-label={label}
      aria-orientation={orientation}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      aria-disabled={disabled}
      tabIndex={disabled ? -1 : 0}
      onKeyDown={onKeyDown}
      onPointerDown={(e) => {
        if (disabled) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        e.currentTarget.focus();
        fromPointer(e);
      }}
      onPointerMove={(e) => {
        if (e.currentTarget.hasPointerCapture(e.pointerId)) fromPointer(e);
      }}
      className={cn(
        "focus-ring group/slider relative flex shrink-0 touch-none select-none items-center rounded-full",
        vertical ? "h-full w-4 cursor-ns-resize justify-center" : "h-4 w-full cursor-ew-resize",
        disabled && "pointer-events-none opacity-40",
        className,
      )}
      {...rest}
    >
      {/* track */}
      <div
        className="relative rounded-full bg-alpha/10"
        style={vertical ? { width: thick, height: "100%" } : { height: thick, width: "100%" }}
      >
        {fill ? (
          <div
            className="absolute rounded-full bg-(--accent,var(--color-gray-950))"
            style={
              vertical
                ? { left: 0, right: 0, bottom: `${fillStart}%`, top: `${100 - fillEnd}%` }
                : { top: 0, bottom: 0, left: `${fillStart}%`, right: `${100 - fillEnd}%` }
            }
          />
        ) : null}
        {bipolar ? (
          <div
            className="absolute bg-alpha/30"
            style={vertical ? { left: -2, right: -2, height: 1, bottom: `${centre}%` } : { top: -2, bottom: -2, width: 1, left: `${centre}%` }}
          />
        ) : null}
        {modPct !== null ? (
          <div
            aria-hidden
            className="absolute size-2 rounded-full border border-(--accent,var(--color-gray-950)) bg-gray-200"
            style={
              vertical
                ? { left: "50%", bottom: `${modPct}%`, transform: "translate(-50%, 50%)" }
                : { top: "50%", left: `${modPct}%`, transform: "translate(-50%, -50%)" }
            }
          />
        ) : null}
      </div>
      {/* thumb */}
      <div
        aria-hidden
        className={cn(
          "absolute rounded-full bg-gray-950 shadow-[0_1px_3px_rgba(0,0,0,0.5)] transition-transform duration-100 ease-fluid group-hover/slider:scale-110 group-active/slider:scale-95",
          size === "sm" ? "size-2.5" : "size-3.5",
        )}
        style={
          vertical
            ? { left: "50%", bottom: `${pct}%`, transform: "translate(-50%, 50%)" }
            : { top: "50%", left: `${pct}%`, transform: "translate(-50%, -50%)" }
        }
      />
    </div>
  );
}
