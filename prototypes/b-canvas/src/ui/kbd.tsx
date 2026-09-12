import type * as React from "react";
import { cn } from "@/lib/cn";

export function Kbd({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <kbd
      className={cn(
        "tabular inline-flex h-4 min-w-4 items-center justify-center rounded border border-alpha/10 bg-alpha/5 px-1 font-sans font-normal text-2xs text-gray-900",
        className,
      )}
    >
      {children}
    </kbd>
  );
}

const KEY_LABELS: Record<string, string> = {
  mod: "⌘",
  alt: "⌥",
  shift: "⇧",
  ctrl: "⌃",
  enter: "↵",
  space: "␣",
  backspace: "⌫",
};

/** Keys separated by "+", e.g. "mod+k". */
export function KbdShortcut({ shortcut, className }: { shortcut: string; className?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-0.5", className)}>
      {shortcut.split("+").map((key) => (
        <Kbd key={key}>{KEY_LABELS[key.toLowerCase()] ?? key.toUpperCase()}</Kbd>
      ))}
    </span>
  );
}
