import * as React from "react";
import { cn } from "@/lib/cn";

export interface KnobProps {
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  size?: 28 | 36 | 44;
  bipolar?: boolean;
  label?: string;
  id?: string;
  "aria-labelledby"?: string;
  disabled?: boolean;
  className?: string;
}

const START = -135;
const SWEEP = 270;

function arc(cx: number, cy: number, r: number, a0: number, a1: number) {
  const toXY = (a: number) => {
    const rad = ((a - 90) * Math.PI) / 180;
    return [cx + r * Math.cos(rad), cy + r * Math.sin(rad)] as const;
  };
  const [x0, y0] = toXY(a0);
  const [x1, y1] = toXY(a1);
  const large = Math.abs(a1 - a0) > 180 ? 1 : 0;
  const sweep = a1 > a0 ? 1 : 0;
  return `M ${x0} ${y0} A ${r} ${r} 0 ${large} ${sweep} ${x1} ${y1}`;
}

/** Rotary control. Drag vertically; arrows nudge. Uses `--accent` for the value arc. */
export function Knob({ value, onChange, min = 0, max = 1, size = 36, bipolar, label, id, disabled, className, ...rest }: KnobProps) {
  const drag = React.useRef<{ y: number; start: number } | null>(null);
  const t = (value - min) / (max - min);
  const angle = START + t * SWEEP;
  const zero = bipolar ? START + ((0 - min) / (max - min)) * SWEEP : START;
  const r = size / 2 - 3;
  const c = size / 2;
  const clamp = (v: number) => Math.min(max, Math.max(min, v));

  return (
    <div
      role="slider"
      id={id}
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      aria-disabled={disabled}
      tabIndex={disabled ? -1 : 0}
      onKeyDown={(e) => {
        const s = (max - min) / 100;
        if (e.key === "ArrowUp" || e.key === "ArrowRight") onChange(clamp(value + s * (e.shiftKey ? 10 : 1)));
        else if (e.key === "ArrowDown" || e.key === "ArrowLeft") onChange(clamp(value - s * (e.shiftKey ? 10 : 1)));
        else return;
        e.preventDefault();
      }}
      onPointerDown={(e) => {
        if (disabled) return;
        drag.current = { y: e.clientY, start: value };
        e.currentTarget.setPointerCapture(e.pointerId);
        e.currentTarget.focus();
      }}
      onPointerMove={(e) => {
        const d = drag.current;
        if (!d) return;
        const px = (max - min) / (e.shiftKey ? 1500 : 150);
        onChange(clamp(d.start + (d.y - e.clientY) * px));
      }}
      onPointerUp={() => (drag.current = null)}
      className={cn(
        "focus-ring inline-flex shrink-0 cursor-ns-resize touch-none select-none rounded-full",
        disabled && "pointer-events-none opacity-40",
        className,
      )}
      style={{ width: size, height: size }}
      {...rest}
    >
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden>
        <path d={arc(c, c, r, START, START + SWEEP)} fill="none" stroke="var(--color-alpha)" strokeOpacity={0.1} strokeWidth={3} strokeLinecap="round" />
        {Math.abs(angle - zero) > 0.5 ? (
          <path d={arc(c, c, r, Math.min(zero, angle), Math.max(zero, angle))} fill="none" stroke="var(--accent, var(--color-gray-950))" strokeWidth={3} strokeLinecap="round" />
        ) : null}
        <circle cx={c} cy={c} r={r - 5} fill="var(--color-gray-300)" />
        <line
          x1={c}
          y1={c - (r - 5) + 3}
          x2={c}
          y2={c - (r - 5) + Math.max(6, size / 5)}
          stroke="var(--color-gray-950)"
          strokeWidth={2}
          strokeLinecap="round"
          transform={`rotate(${angle} ${c} ${c})`}
        />
      </svg>
    </div>
  );
}
