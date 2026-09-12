import { Check, ChevronDown } from "lucide-react";
import * as React from "react";
import { cn } from "@/lib/cn";
import { Button, KbdShortcut, Separator } from "@/ui";
import { project } from "./project";

/** Top-left of the canvas: the project name. Everything else about the project lives in its menu. */
export function ProjectMenu() {
  const [open, setOpen] = React.useState(false);
  const [device, setDevice] = React.useState(project.device);
  const ref = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("pointerdown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointerdown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div ref={ref} className="relative">
      <Button
        variant="ghost"
        size="md"
        className="gap-1.5 rounded-lg pr-2 pl-2.5 font-medium"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        {project.name}
        <ChevronDown className="size-3.5 text-gray-700" />
      </Button>
      {open ? (
        <div role="menu" aria-label="Project" className="absolute top-full left-0 mt-1 flex w-60 flex-col rounded-lg border border-alpha/10 bg-gray-200 p-1 shadow-dropdown">
          <Item label="Add tool" shortcut="mod+k" onSelect={() => setOpen(false)} />
          <Item label="Undo" shortcut="mod+z" onSelect={() => setOpen(false)} />
          <Item label="Redo" shortcut="mod+shift+z" disabled />
          <Separator className="my-1" />
          <span className="px-2 pt-1 pb-1 text-2xs text-gray-700">Output</span>
          {project.devices.map((d) => (
            <Item key={d} label={d} checked={d === device} onSelect={() => setDevice(d)} />
          ))}
          <Separator className="my-1" />
          <Item label="Show project folder" onSelect={() => setOpen(false)} />
        </div>
      ) : null}
    </div>
  );
}

function Item({
  label,
  shortcut,
  checked,
  disabled,
  onSelect,
}: {
  label: string;
  shortcut?: string;
  checked?: boolean;
  disabled?: boolean;
  onSelect?: () => void;
}) {
  return (
    <button
      type="button"
      role={checked === undefined ? "menuitem" : "menuitemradio"}
      aria-checked={checked}
      disabled={disabled}
      onClick={onSelect}
      className={cn(
        "focus-ring flex h-8 items-center gap-2 rounded-md px-2 text-left text-sm transition-colors duration-100 hover:bg-alpha/5 disabled:pointer-events-none disabled:opacity-40",
      )}
    >
      <span className="flex-1 truncate">{label}</span>
      {checked ? <Check className="size-3.5 text-gray-800" /> : null}
      {shortcut ? <KbdShortcut shortcut={shortcut} /> : null}
    </button>
  );
}
