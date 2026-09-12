import * as React from "react";
import { cn } from "./cn";

/**
 * Small, dependency-free tooltip. Shows on hover and on keyboard focus of the child.
 * The child must accept `aria-describedby`.
 */
export function Tooltip({
  content,
  side = "top",
  children,
  className,
  delay = 400,
}: {
  content: React.ReactNode;
  side?: "top" | "bottom" | "left" | "right";
  children: React.ReactElement<Record<string, unknown>>;
  className?: string;
  delay?: number;
}) {
  const id = React.useId();
  const [open, setOpen] = React.useState(false);
  const timer = React.useRef<number | null>(null);

  const show = () => {
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setOpen(true), delay);
  };
  const hide = () => {
    if (timer.current) window.clearTimeout(timer.current);
    setOpen(false);
  };

  const positions: Record<typeof side, string> = {
    top: "bottom-full left-1/2 mb-1.5 -translate-x-1/2",
    bottom: "top-full left-1/2 mt-1.5 -translate-x-1/2",
    left: "right-full top-1/2 mr-1.5 -translate-y-1/2",
    right: "left-full top-1/2 ml-1.5 -translate-y-1/2",
  };

  return (
    <span
      className="relative inline-flex"
      onPointerEnter={show}
      onPointerLeave={hide}
      onFocusCapture={show}
      onBlurCapture={hide}
    >
      {React.cloneElement(children, { "aria-describedby": open ? id : undefined })}
      {open && (
        <span
          role="tooltip"
          id={id}
          className={cn(
            "pointer-events-none absolute z-50 flex h-6 items-center gap-1.5 whitespace-nowrap rounded-md bg-gray-300 px-2 text-xs font-medium text-gray-950 shadow-popup",
            positions[side],
            className,
          )}
        >
          {content}
        </span>
      )}
    </span>
  );
}
