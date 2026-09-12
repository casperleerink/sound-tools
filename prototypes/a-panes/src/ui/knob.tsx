import { cn } from "./cn";
import { useDragValue } from "./use-drag-value";

export interface KnobProps {
  id?: string;
  value: number;
  onChange: (v: number) => void;
  min: number;
  max: number;
  step?: number;
  label: string;
  size?: 28 | 36 | 44;
  /** Bipolar knobs fill from the centre. */
  origin?: number;
  disabled?: boolean;
  className?: string;
  color?: string;
}

const SWEEP = 270; // degrees of travel
const START = -135;

function polar(cx: number, cy: number, r: number, deg: number) {
  const a = ((deg - 90) * Math.PI) / 180;
  return { x: cx + r * Math.cos(a), y: cy + r * Math.sin(a) };
}

function arc(cx: number, cy: number, r: number, a0: number, a1: number) {
  const s = polar(cx, cy, r, a0);
  const e = polar(cx, cy, r, a1);
  const large = Math.abs(a1 - a0) > 180 ? 1 : 0;
  const sweep = a1 > a0 ? 1 : 0;
  return `M ${s.x} ${s.y} A ${r} ${r} 0 ${large} ${sweep} ${e.x} ${e.y}`;
}

/** Small rotary control. Drag vertically; shift for fine; arrow keys nudge. */
export function Knob({
  id,
  value,
  onChange,
  min,
  max,
  step,
  label,
  size = 36,
  origin,
  disabled,
  className,
  color = "var(--accent)",
}: KnobProps) {
  const { dragging, handlers } = useDragValue({ value, min, max, step, onChange, pixelsForRange: 180 });
  const t = (value - min) / (max - min);
  const t0 = ((origin ?? min) - min) / (max - min);
  const a = START + t * SWEEP;
  const a0 = START + t0 * SWEEP;
  const c = size / 2;
  const r = c - 3;
  const tip = polar(c, c, r - 5, a);
  const fillPath = Math.abs(a - a0) < 0.5 ? "" : arc(c, c, r, Math.min(a, a0), Math.max(a, a0));

  return (
    <div
      id={id}
      role="slider"
      tabIndex={disabled ? -1 : 0}
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      aria-disabled={disabled}
      {...handlers}
      className={cn(
        "inline-flex shrink-0 cursor-ns-resize touch-none select-none rounded-full",
        disabled && "cursor-not-allowed opacity-40",
        className,
      )}
      style={{ width: size, height: size }}
    >
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden>
        <path
          d={arc(c, c, r, START, START + SWEEP)}
          fill="none"
          stroke="color-mix(in oklab, var(--color-alpha) 10%, transparent)"
          strokeWidth={3}
          strokeLinecap="round"
        />
        {fillPath && <path d={fillPath} fill="none" stroke={color} strokeWidth={3} strokeLinecap="round" />}
        <circle
          cx={c}
          cy={c}
          r={r - 6}
          fill={dragging ? "var(--color-gray-400)" : "var(--color-gray-300)"}
          className="transition-colors duration-100"
        />
        <line
          x1={c}
          y1={c}
          x2={tip.x}
          y2={tip.y}
          stroke="var(--color-gray-950)"
          strokeWidth={2}
          strokeLinecap="round"
        />
      </svg>
    </div>
  );
}
