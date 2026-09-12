import * as React from "react";
import { cn } from "@/lib/cn";

export interface FieldProps {
  label: string;
  /** Right-aligned readout, e.g. the current value. */
  value?: React.ReactNode;
  /** Small hint under the control. */
  hint?: React.ReactNode;
  /** "stack" puts the label above the control; "row" puts them side by side. */
  layout?: "stack" | "row";
  className?: string;
  children: React.ReactElement<{ id?: string; "aria-labelledby"?: string }>;
}

/** Labelled control. Wires the label to the control by id. */
export function Field({ label, value, hint, layout = "stack", className, children }: FieldProps) {
  const id = React.useId();
  const labelId = `${id}-label`;
  const control = React.cloneElement(children, { id, "aria-labelledby": labelId });
  if (layout === "row") {
    return (
      <div className={cn("flex items-center gap-3", className)}>
        <span id={labelId} className="w-16 shrink-0 truncate font-medium text-gray-900 text-xs">
          {label}
        </span>
        <div className="min-w-0 flex-1">{control}</div>
        {value !== undefined ? <span className="tabular shrink-0 text-gray-900 text-xs">{value}</span> : null}
      </div>
    );
  }
  return (
    <div className={cn("flex flex-col gap-1.5", className)}>
      <div className="flex items-baseline justify-between gap-2">
        <span id={labelId} className="truncate font-medium text-gray-900 text-xs">
          {label}
        </span>
        {value !== undefined ? <span className="tabular shrink-0 text-gray-800 text-xs">{value}</span> : null}
      </div>
      {control}
      {hint ? <span className="text-2xs text-gray-700">{hint}</span> : null}
    </div>
  );
}
