import * as React from "react";
import { cn } from "./cn";

/**
 * Label + control + optional value readout, in one of two layouts.
 * `row`: label left, control right (dense settings). `stack`: label above control (parameters).
 * Pass `htmlFor` to link the label to the control's id.
 */
export function LabelledControl({
  label,
  hint,
  value,
  htmlFor,
  layout = "stack",
  className,
  children,
}: {
  label: string;
  /** Muted text after the label (unit or range). */
  hint?: string;
  /** Readout shown on the right, tabular. */
  value?: React.ReactNode;
  htmlFor?: string;
  layout?: "row" | "stack";
  className?: string;
  children: React.ReactNode;
}) {
  const head = (
    <div className="flex min-w-0 items-baseline justify-between gap-2">
      <label htmlFor={htmlFor} className="truncate text-xs font-medium text-gray-950/70">
        {label}
        {hint && <span className="ml-1 font-normal text-gray-950/40">{hint}</span>}
      </label>
      {value !== undefined && (
        <span className="shrink-0 text-xs font-medium text-gray-950 tabular">{value}</span>
      )}
    </div>
  );
  if (layout === "row") {
    return (
      <div className={cn("flex h-8 items-center justify-between gap-3", className)}>
        <label htmlFor={htmlFor} className="truncate text-sm font-medium text-gray-950/80">
          {label}
          {hint && <span className="ml-1 text-xs font-normal text-gray-950/40">{hint}</span>}
        </label>
        <div className="flex shrink-0 items-center gap-2">
          {value !== undefined && <span className="text-xs text-gray-950/60 tabular">{value}</span>}
          {children}
        </div>
      </div>
    );
  }
  return (
    <div className={cn("flex flex-col gap-1.5", className)}>
      {head}
      {children}
    </div>
  );
}

/** Group of controls with a small uppercase heading. */
export function ControlGroup({
  title,
  action,
  className,
  children,
}: {
  title?: string;
  action?: React.ReactNode;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <section className={cn("flex flex-col gap-3", className)}>
      {(title || action) && (
        <div className="flex h-5 items-center justify-between">
          {title && (
            <h3 className="text-2xs font-semibold uppercase tracking-[0.08em] text-gray-950/40">{title}</h3>
          )}
          {action}
        </div>
      )}
      {children}
    </section>
  );
}
