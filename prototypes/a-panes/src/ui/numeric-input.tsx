import * as React from "react";
import { cn } from "./cn";
import { clamp, snap, useDragValue } from "./use-drag-value";

export interface NumericInputProps {
  id?: string;
  value: number;
  onChange: (v: number) => void;
  min: number;
  max: number;
  step?: number;
  decimals?: number;
  unit?: string;
  /** Accessible label; use when there is no visible label. */
  label?: string;
  size?: "xs" | "sm" | "md";
  /** Drag direction to change the value. Default vertical. */
  axis?: "x" | "y";
  disabled?: boolean;
  className?: string;
  /** Optional custom formatting of the displayed value (not while editing). */
  format?: (v: number) => string;
}

/**
 * Number box: drag up/down to change, click to type, arrow keys to nudge, shift for fine.
 */
export function NumericInput({
  id,
  value,
  onChange,
  min,
  max,
  step,
  decimals = step && step < 1 ? (step.toString().split(".")[1] ?? "").length : 0,
  unit,
  label,
  size = "sm",
  axis = "y",
  disabled,
  className,
  format,
}: NumericInputProps) {
  const [editing, setEditing] = React.useState(false);
  const [text, setText] = React.useState("");
  const inputRef = React.useRef<HTMLInputElement>(null);
  const moved = React.useRef(false);

  const { dragging, handlers } = useDragValue({ value, min, max, step, axis, onChange, pixelsForRange: 200 });

  const commitText = () => {
    const n = Number.parseFloat(text.replace(",", "."));
    if (!Number.isNaN(n)) onChange(clamp(snap(n, step, min), min, max));
    setEditing(false);
  };

  const display = format ? format(value) : value.toFixed(decimals);
  const h = size === "xs" ? "h-6 text-xs" : size === "sm" ? "h-7 text-xs" : "h-8 text-sm";

  if (editing) {
    return (
      <input
        ref={inputRef}
        id={id}
        aria-label={label}
        autoFocus
        inputMode="decimal"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={commitText}
        onKeyDown={(e) => {
          if (e.key === "Enter") commitText();
          if (e.key === "Escape") setEditing(false);
        }}
        className={cn(
          "w-full min-w-0 shrink-0 rounded-md border border-lavender-500 bg-gray-50 px-2 text-right font-medium text-gray-950 tabular outline-none",
          h,
          className,
        )}
      />
    );
  }

  return (
    <button
      id={id}
      type="button"
      role="spinbutton"
      aria-label={label}
      aria-valuenow={value}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuetext={unit ? `${display} ${unit}` : display}
      disabled={disabled}
      {...handlers}
      onPointerDown={(e) => {
        moved.current = false;
        handlers.onPointerDown(e);
      }}
      onPointerMove={(e) => {
        if (e.buttons) moved.current = true;
        handlers.onPointerMove(e);
      }}
      onClick={() => {
        if (moved.current) return;
        setText(value.toFixed(decimals));
        setEditing(true);
      }}
      className={cn(
        "inline-flex w-full min-w-0 shrink-0 items-center justify-end gap-1 rounded-md border border-alpha/10 bg-alpha/5 px-2 font-medium text-gray-950 tabular transition-colors duration-100 hover:bg-alpha/10 disabled:cursor-not-allowed disabled:opacity-40",
        axis === "y" ? "cursor-ns-resize" : "cursor-ew-resize",
        dragging && "border-(--accent)/60 bg-alpha/10",
        h,
        className,
      )}
    >
      <span className="truncate">{display}</span>
      {unit && <span className="shrink-0 font-normal text-gray-950/50">{unit}</span>}
    </button>
  );
}
