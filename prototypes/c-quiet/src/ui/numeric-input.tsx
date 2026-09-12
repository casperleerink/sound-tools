import * as React from "react";
import { cn } from "@/lib/cn";

export interface NumericInputProps {
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  /** Value change per pixel of drag. Defaults to (max-min)/200 or 1. */
  step?: number;
  /** Decimal places shown. */
  precision?: number;
  unit?: string;
  size?: "xs" | "sm" | "md";
  /** Accessible name when not wrapped in a Field. */
  label?: string;
  id?: string;
  "aria-labelledby"?: string;
  disabled?: boolean;
  /** Stretch to the parent width. */
  block?: boolean;
  className?: string;
}

const sizes = {
  xs: "h-6 rounded-md px-1.5 text-xs",
  sm: "h-7 rounded-lg px-2 text-sm",
  md: "h-8 rounded-lg px-2.5 text-sm",
};

/** Number field. Drag up/down or left/right to change, click to type, arrows to nudge. */
export function NumericInput({
  value,
  onChange,
  min = -Infinity,
  max = Infinity,
  step,
  precision = 0,
  unit,
  size = "sm",
  label,
  id,
  disabled,
  block,
  className,
  ...rest
}: NumericInputProps) {
  const [editing, setEditing] = React.useState(false);
  const [text, setText] = React.useState("");
  const drag = React.useRef<{ x: number; y: number; start: number; moved: boolean } | null>(null);
  const inputRef = React.useRef<HTMLInputElement>(null);

  const range = Number.isFinite(max - min) ? max - min : null;
  const perPixel = step ?? (range ? range / 200 : 1);
  const clamp = (v: number) => Math.min(max, Math.max(min, v));
  const format = (v: number) => v.toFixed(precision);

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (disabled || editing) return;
    drag.current = { x: e.clientX, y: e.clientY, start: value, moved: false };
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    const dx = e.clientX - d.x;
    const dy = d.y - e.clientY;
    const delta = Math.abs(dx) > Math.abs(dy) ? dx : dy;
    if (Math.abs(delta) > 2) d.moved = true;
    if (!d.moved) return;
    const fine = e.shiftKey ? 0.1 : 1;
    const next = clamp(d.start + delta * perPixel * fine);
    onChange(Number(next.toFixed(Math.max(precision, 3))));
  };
  const onPointerUp = () => {
    const d = drag.current;
    drag.current = null;
    if (d && !d.moved) {
      setText(format(value));
      setEditing(true);
      requestAnimationFrame(() => inputRef.current?.select());
    }
  };
  const commit = () => {
    const n = Number.parseFloat(text);
    if (!Number.isNaN(n)) onChange(clamp(n));
    setEditing(false);
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    const nudge = (e.shiftKey ? 10 : 1) * (precision > 0 ? 10 ** -precision : 1);
    if (e.key === "ArrowUp") {
      onChange(clamp(value + nudge));
      e.preventDefault();
    } else if (e.key === "ArrowDown") {
      onChange(clamp(value - nudge));
      e.preventDefault();
    } else if (e.key === "Enter" && !editing) {
      setText(format(value));
      setEditing(true);
      requestAnimationFrame(() => inputRef.current?.select());
    }
  };

  return (
    <div
      role={editing ? undefined : "spinbutton"}
      id={editing ? undefined : id}
      aria-label={label}
      aria-valuenow={value}
      aria-valuemin={Number.isFinite(min) ? min : undefined}
      aria-valuemax={Number.isFinite(max) ? max : undefined}
      aria-valuetext={`${format(value)}${unit ? ` ${unit}` : ""}`}
      aria-disabled={disabled}
      tabIndex={editing || disabled ? -1 : 0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onKeyDown={editing ? undefined : onKeyDown}
      className={cn(
        "focus-ring tabular inline-flex items-center justify-between gap-1 border border-alpha/10 bg-alpha/5 font-medium text-gray-950 transition-colors duration-100 hover:bg-alpha/10",
        !editing && !disabled && "cursor-ns-resize",
        disabled && "pointer-events-none opacity-40",
        block ? "w-full" : "min-w-16",
        sizes[size],
        className,
      )}
      {...rest}
    >
      {editing ? (
        <input
          ref={inputRef}
          id={id}
          aria-label={label}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") commit();
            if (e.key === "Escape") setEditing(false);
          }}
          autoFocus
          inputMode="decimal"
          className="tabular w-full min-w-0 bg-transparent font-medium text-gray-950 outline-none"
        />
      ) : (
        <span className="truncate">{format(value)}</span>
      )}
      {unit ? <span className="shrink-0 text-gray-700">{unit}</span> : null}
    </div>
  );
}
