import { Copy, X } from "lucide-react";
import * as React from "react";
import { cn } from "@/lib/cn";
import { accentStyle, type Accent } from "@/lib/accent";
import type { PortDef, PortKind } from "@/core/sdk";
import { Badge } from "./badge";
import { IconButton } from "./button";
import { SegmentedControl } from "./segmented-control";
import { Tooltip } from "./tooltip";

/* Geometry shared with the connection layer so ports and curves line up. */
export const TOOL_HEADER_H = 36;
export const PORT_TOP = TOOL_HEADER_H + 18;
export const PORT_GAP = 24;
export const portOffsetY = (index: number) => PORT_TOP + index * PORT_GAP;

export interface ToolFrameProps {
  instanceName: string;
  typeName: string;
  accent: Accent;
  views: { id: string; label: string }[];
  activeView: string;
  onViewChange: (id: string) => void;
  onOpenView?: () => void;
  onClose?: () => void;
  /** Number of open cards for this instance. > 1 shows the shared marker. */
  openViews?: number;
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

/** Card frame for one view of a tool instance: header, ports on the edges, body. */
export function ToolFrame({
  instanceName,
  typeName,
  accent,
  views,
  activeView,
  onViewChange,
  onOpenView,
  onClose,
  openViews = 1,
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
      style={{ ...accentStyle(accent), width, ...style }}
      className={cn(
        "relative flex flex-col rounded-[10px] border bg-gray-200 text-gray-950 shadow-card transition-[border-color,box-shadow] duration-100",
        selected ? "border-(--accent)/60 shadow-[0_0_0_1px_var(--accent),var(--shadow-card)]" : "border-alpha/10",
        className,
      )}
    >
      <header
        onPointerDown={onHeaderPointerDown}
        className="flex h-9 shrink-0 cursor-grab items-center gap-1.5 border-alpha/5 border-b pr-1.5 pl-3 active:cursor-grabbing"
      >
        <span aria-hidden className="size-2 shrink-0 rounded-full bg-(--accent)" />
        <span className="min-w-10 truncate font-medium text-sm">{instanceName}</span>
        {width >= 400 ? <span className="truncate text-gray-700 text-xs">{typeName}</span> : null}
        {openViews > 1 ? (
          <Tooltip content={`${openViews} views of this instance are open`}>
            <Badge variant="accent-outline" size="xs" icon={<Copy />}>
              {openViews}
            </Badge>
          </Tooltip>
        ) : null}
        <span className="flex-1" />
        {views.length > 1 ? (
          <SegmentedControl
            size="xs"
            label={`${instanceName} view`}
            value={activeView}
            onValueChange={onViewChange}
            options={views.map((v) => ({ value: v.id, label: v.label }))}
          />
        ) : (
          <span className="pr-1 text-gray-700 text-xs">{views[0]?.label}</span>
        )}
        <IconButton label="Open another view" size="xs" variant="ghost-muted" onClick={onOpenView}>
          <Copy />
        </IconButton>
        <IconButton label="Close view" size="xs" variant="ghost-muted" onClick={onClose}>
          <X />
        </IconButton>
      </header>
      <div className="min-h-0 p-3">{children}</div>
      {inputs.map((p, i) => (
        <Port key={p.id} port={p} side="in" y={portOffsetY(i)} connected={connectedPorts?.has(p.id) ?? false} />
      ))}
      {outputs.map((p, i) => (
        <Port key={p.id} port={p} side="out" y={portOffsetY(i)} connected={connectedPorts?.has(p.id) ?? false} />
      ))}
    </section>
  );
}

const kindLabel: Record<PortKind, string> = { audio: "audio", event: "events", mod: "modulation" };

export function Port({ port, side, y, connected }: { port: PortDef; side: "in" | "out"; y: number; connected: boolean }) {
  return (
    <div
      className={cn("group/port absolute flex h-4 items-center", side === "in" ? "-left-2 flex-row" : "-right-2 flex-row-reverse")}
      style={{ top: y - 8 }}
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
  const base = cn(
    "block border-2 border-(--accent) transition-transform duration-100 group-hover/port:scale-125",
    connected ? "" : "opacity-60",
    className,
  );
  if (kind === "audio") return <span aria-hidden className={cn(base, "size-2.5 rounded-full bg-(--accent)")} />;
  if (kind === "event") return <span aria-hidden className={cn(base, "size-2.5 rounded-full bg-gray-200")} />;
  return <span aria-hidden className={cn(base, "size-2 rotate-45 rounded-[1px] bg-(--accent)")} />;
}
