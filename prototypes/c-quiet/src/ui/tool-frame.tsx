import { X } from "lucide-react";
import * as React from "react";
import { cn } from "@/lib/cn";
import { accentStyle, type Accent } from "@/lib/accent";
import type { PortDef, PortKind } from "@/core/sdk";
import { IconButton } from "./button";

/* Geometry shared with the wire layer so ports and curves line up. */
export const TOOL_HEADER_H = 44;
export const PORT_TOP = TOOL_HEADER_H + 20;
export const PORT_GAP = 24;
export const portOffsetY = (index: number) => PORT_TOP + index * PORT_GAP;

export interface ToolFrameProps {
  instanceName: string;
  typeName: string;
  accent: Accent;
  views: { id: string; label: string }[];
  activeView: string;
  onViewChange: (id: string) => void;
  onClose?: () => void;
  inputs?: PortDef[];
  outputs?: PortDef[];
  connectedPorts?: ReadonlySet<string>;
  width: number;
  selected?: boolean;
  /** Header drag handle. */
  onHeaderPointerDown?: (e: React.PointerEvent<HTMLDivElement>) => void;
  className?: string;
  style?: React.CSSProperties;
  children: React.ReactNode;
}

/** Card frame for one view of a tool instance: header, ports on the edges, body.
   The instance accent colours the dot and the ports only; the body stays neutral. */
export function ToolFrame({
  instanceName,
  typeName,
  accent,
  views,
  activeView,
  onViewChange,
  onClose,
  inputs = [],
  outputs = [],
  connectedPorts,
  width,
  selected,
  onHeaderPointerDown,
  className,
  style,
  children,
}: ToolFrameProps) {
  return (
    <section
      aria-label={`${instanceName} (${typeName})`}
      style={{ width, ...style }}
      className={cn(
        "group/card relative flex flex-col rounded-[10px] border bg-gray-200 text-gray-950 shadow-card transition-[border-color] duration-100",
        selected ? "border-alpha/25" : "border-alpha/10",
        className,
      )}
    >
      <header
        onPointerDown={onHeaderPointerDown}
        className="flex shrink-0 cursor-grab items-center gap-2.5 pr-3 pl-4 active:cursor-grabbing"
        style={{ height: TOOL_HEADER_H }}
      >
        <span aria-hidden className="size-2 shrink-0 rounded-full bg-(--accent)" style={accentStyle(accent)} />
        <span className="min-w-10 truncate font-medium text-sm">{instanceName}</span>
        <span className="flex-1" />
        {views.length > 1 ? (
          <div role="tablist" aria-label={`${instanceName} view`} className="flex items-center gap-1">
            {views.map((v) => {
              const active = v.id === activeView;
              return (
                <button
                  key={v.id}
                  type="button"
                  role="tab"
                  aria-selected={active}
                  onClick={() => onViewChange(v.id)}
                  className={cn(
                    "focus-ring h-7 rounded-md px-2 font-medium text-xs transition-opacity duration-100 hover:opacity-100",
                    active ? "opacity-100" : "opacity-40",
                  )}
                >
                  {v.label}
                </button>
              );
            })}
          </div>
        ) : null}
        {onClose ? (
          <IconButton
            label="Close view"
            size="xs"
            variant="ghost-muted"
            onClick={onClose}
            className="opacity-0 transition-opacity duration-100 focus-visible:opacity-100 group-hover/card:opacity-100"
          >
            <X />
          </IconButton>
        ) : null}
      </header>
      <div className="min-h-0 px-4 pt-1 pb-4">{children}</div>
      {inputs.map((p, i) => (
        <Port key={p.id} port={p} accent={accent} side="in" y={portOffsetY(i)} connected={connectedPorts?.has(p.id) ?? false} />
      ))}
      {outputs.map((p, i) => (
        <Port key={p.id} port={p} accent={accent} side="out" y={portOffsetY(i)} connected={connectedPorts?.has(p.id) ?? false} />
      ))}
    </section>
  );
}

const kindLabel: Record<PortKind, string> = { audio: "audio", event: "events", mod: "modulation" };

export function Port({ port, accent, side, y, connected }: { port: PortDef; accent: Accent; side: "in" | "out"; y: number; connected: boolean }) {
  return (
    <div
      className={cn("group/port absolute flex h-4 items-center", side === "in" ? "-left-2 flex-row" : "-right-2 flex-row-reverse")}
      style={{ top: y - 8, ...accentStyle(accent) }}
    >
      <button
        type="button"
        aria-label={`${port.label} ${kindLabel[port.kind]} ${side === "in" ? "input" : "output"}`}
        className="focus-ring flex size-4 items-center justify-center rounded-full"
      >
        <PortGlyph kind={port.kind} connected={connected} />
      </button>
      <span
        className={cn(
          "pointer-events-none whitespace-nowrap rounded-md border border-alpha/10 bg-gray-300 px-1.5 py-0.5 font-medium text-2xs text-gray-950 opacity-0 shadow-dropdown transition-opacity duration-100 group-focus-within/port:opacity-100 group-hover/port:opacity-100",
          side === "in" ? "ml-0.5" : "mr-0.5",
        )}
      >
        {port.label}
        <span className="ml-1 text-gray-700">{kindLabel[port.kind]}</span>
      </span>
    </div>
  );
}

/** audio = filled disc, event = ring, mod = diamond. Unconnected ports are dimmer. */
export function PortGlyph({ kind, connected, className }: { kind: PortKind; connected: boolean; className?: string }) {
  const base = cn("block border-2 border-(--accent) transition-transform duration-100 group-hover/port:scale-125", connected ? "" : "opacity-40", className);
  if (kind === "audio") return <span aria-hidden className={cn(base, "size-2.5 rounded-full bg-(--accent)")} />;
  if (kind === "event") return <span aria-hidden className={cn(base, "size-2.5 rounded-full bg-gray-200")} />;
  return <span aria-hidden className={cn(base, "size-2 rotate-45 rounded-[1px] bg-(--accent)")} />;
}
