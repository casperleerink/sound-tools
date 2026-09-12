import * as React from "react";

export interface DragValueOptions {
  value: number;
  min: number;
  max: number;
  step?: number;
  /** Pixels of pointer travel for the full range. Default 160. */
  pixelsForRange?: number;
  axis?: "x" | "y" | "both";
  onChange: (value: number) => void;
  onCommit?: () => void;
}

export function clamp(v: number, min: number, max: number) {
  return Math.min(max, Math.max(min, v));
}

export function snap(v: number, step: number | undefined, min: number) {
  if (!step) return v;
  const n = Math.round((v - min) / step);
  const out = min + n * step;
  // Avoid floating drift like 0.30000000000000004.
  const decimals = (step.toString().split(".")[1] ?? "").length;
  return Number(out.toFixed(decimals));
}

/**
 * Pointer drag that turns pixel travel into a value change. Shift = fine (x0.1).
 * Returns handlers for the draggable element and whether a drag is active.
 */
export function useDragValue(opts: DragValueOptions) {
  const { value, min, max, step, pixelsForRange = 160, axis = "y", onChange } = opts;
  const [dragging, setDragging] = React.useState(false);
  const start = React.useRef<{ x: number; y: number; value: number } | null>(null);
  const latest = React.useRef(opts);
  latest.current = opts;

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    start.current = { x: e.clientX, y: e.clientY, value };
    setDragging(true);
    e.preventDefault();
  };
  const onPointerMove = (e: React.PointerEvent) => {
    if (!start.current) return;
    const dx = e.clientX - start.current.x;
    const dy = start.current.y - e.clientY;
    const delta = axis === "x" ? dx : axis === "y" ? dy : dx + dy;
    const fine = e.shiftKey ? 0.1 : 1;
    const next = start.current.value + (delta / pixelsForRange) * (max - min) * fine;
    onChange(clamp(snap(next, step, min), min, max));
  };
  const onPointerUp = () => {
    if (!start.current) return;
    start.current = null;
    setDragging(false);
    latest.current.onCommit?.();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const s = step ?? (max - min) / 100;
    const big = s * 10;
    let next: number | null = null;
    if (e.key === "ArrowUp" || e.key === "ArrowRight") next = value + (e.shiftKey ? big : s);
    if (e.key === "ArrowDown" || e.key === "ArrowLeft") next = value - (e.shiftKey ? big : s);
    if (e.key === "Home") next = min;
    if (e.key === "End") next = max;
    if (e.key === "PageUp") next = value + big;
    if (e.key === "PageDown") next = value - big;
    if (next !== null) {
      e.preventDefault();
      onChange(clamp(snap(next, step ?? s, min), min, max));
    }
  };

  return {
    dragging,
    handlers: { onPointerDown, onPointerMove, onPointerUp, onPointerCancel: onPointerUp, onKeyDown },
  };
}

export function formatValue(v: number, opts: { decimals?: number; unit?: string } = {}) {
  const { decimals = 0, unit = "" } = opts;
  const s = v.toFixed(decimals);
  return unit ? `${s}${unit === "%" || unit === "°" || unit === "×" ? "" : " "}${unit}` : s;
}
