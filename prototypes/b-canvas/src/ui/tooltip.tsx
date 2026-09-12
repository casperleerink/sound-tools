import * as React from "react";
import { cn } from "@/lib/cn";
import { KbdShortcut } from "./kbd";

type Side = "top" | "bottom" | "left" | "right";

const sideClasses: Record<Side, string> = {
  top: "bottom-full left-1/2 mb-1.5 -translate-x-1/2",
  bottom: "top-full left-1/2 mt-1.5 -translate-x-1/2",
  left: "right-full top-1/2 mr-1.5 -translate-y-1/2",
  right: "left-full top-1/2 ml-1.5 -translate-y-1/2",
};

export interface TooltipProps {
  content: React.ReactNode;
  shortcut?: string;
  side?: Side;
  /** Force the tooltip visible (for the gallery). */
  open?: boolean;
  className?: string;
  children: React.ReactElement;
}

/** Hover/focus tooltip. Pure CSS so it stays cheap; the wrapper is inline-flex. */
export function Tooltip({ content, shortcut, side = "top", open, className, children }: TooltipProps) {
  const id = React.useId();
  return (
    <span className={cn("group/tip relative inline-flex", className)}>
      {React.cloneElement(children as React.ReactElement<{ "aria-describedby"?: string }>, {
        "aria-describedby": id,
      })}
      <span
        role="tooltip"
        id={id}
        data-open={open ? "" : undefined}
        className={cn(
          "pointer-events-none absolute z-50 flex h-6 items-center gap-1.5 whitespace-nowrap rounded-md border border-alpha/10 bg-gray-300 px-2 font-medium text-gray-950 text-xs opacity-0 shadow-dropdown transition-opacity delay-300 duration-100 group-focus-within/tip:opacity-100 group-hover/tip:opacity-100 data-open:opacity-100",
          sideClasses[side],
        )}
      >
        {content}
        {shortcut ? <KbdShortcut shortcut={shortcut} /> : null}
      </span>
    </span>
  );
}
