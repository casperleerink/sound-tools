import { cn } from "./cn";

export function Kbd({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <kbd
      className={cn(
        "inline-flex h-4 min-w-4 items-center justify-center rounded border border-alpha/5 bg-alpha/5 px-1 font-sans text-2xs font-medium text-gray-950/70 tabular",
        className,
      )}
    >
      {children}
    </kbd>
  );
}

const KEY_LABELS: Record<string, string> = {
  mod: "⌘",
  cmd: "⌘",
  alt: "⌥",
  shift: "⇧",
  ctrl: "⌃",
  enter: "↵",
  space: "␣",
  backspace: "⌫",
  esc: "esc",
};

/** Keys separated by "+", e.g. "mod+enter". */
export function KbdShortcut({ shortcut, className }: { shortcut: string; className?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-0.5", className)}>
      {shortcut.split("+").map((key) => (
        <Kbd key={key}>{KEY_LABELS[key.toLowerCase()] ?? key.toUpperCase()}</Kbd>
      ))}
    </span>
  );
}
