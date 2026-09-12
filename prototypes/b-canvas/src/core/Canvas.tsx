import { Maximize2, Minus, Plus } from "lucide-react";
import * as React from "react";
import { getTool } from "@/extensions";
import { cn } from "@/lib/cn";
import { accents } from "@/lib/accent";
import { Button, IconButton, Separator, ToolFrame, Tooltip, portOffsetY } from "@/ui";
import type { PortKind } from "./sdk";
import { type CardPlacement, type Connection, connections as initialConnections, getInstance, initialCards } from "./project";

const ZOOM_STEPS = [0.5, 0.6, 0.75, 0.9, 1, 1.25, 1.5];
/* Room the floating chrome needs when fitting: top-left cluster, transport pill, side gutters. */
const FIT_INSET = { top: 56, right: 24, bottom: 72, left: 24 };

interface Point {
  x: number;
  y: number;
}

/* Pannable workspace. Cards are absolutely positioned in canvas space; an SVG
   layer in the same space draws connections between ports. */
export function Canvas() {
  const [cards, setCards] = React.useState<CardPlacement[]>(initialCards);
  const [pan, setPan] = React.useState<Point>({ x: 0, y: 0 });
  const [zoom, setZoom] = React.useState(1);
  const [selected, setSelected] = React.useState<string | null>("c1");
  const rootRef = React.useRef<HTMLDivElement>(null);

  /** Zoom and pan so every card is visible, at most 100%, in 5% steps. */
  const fit = React.useCallback(() => {
    const root = rootRef.current;
    if (!root) return;
    const els = Array.from(root.querySelectorAll<HTMLElement>("[data-card-x]"));
    if (els.length === 0) return;
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const el of els) {
      const x = Number(el.dataset.cardX);
      const y = Number(el.dataset.cardY);
      minX = Math.min(minX, x - 8);
      minY = Math.min(minY, y);
      maxX = Math.max(maxX, x + el.offsetWidth + 8);
      maxY = Math.max(maxY, y + el.offsetHeight);
    }
    const availW = root.clientWidth - FIT_INSET.left - FIT_INSET.right;
    const availH = root.clientHeight - FIT_INSET.top - FIT_INSET.bottom;
    const raw = Math.min(1, availW / (maxX - minX), availH / (maxY - minY));
    const z = Math.max(0.5, Math.round(raw * 20) / 20);
    setZoom(z);
    setPan({
      x: FIT_INSET.left + (availW - (maxX - minX) * z) / 2 - minX * z,
      y: FIT_INSET.top + (availH - (maxY - minY) * z) / 2 - minY * z,
    });
  }, []);
  React.useLayoutEffect(fit, [fit]);
  const zoomBy = (dir: 1 | -1) => {
    const i = ZOOM_STEPS.findIndex((z) => z >= zoom - 0.001);
    const next = ZOOM_STEPS[Math.min(ZOOM_STEPS.length - 1, Math.max(0, i + dir))] ?? zoom;
    setZoom(next);
  };
  const drag = React.useRef<{ kind: "pan" | "card"; id?: string; start: Point; origin: Point } | null>(null);

  const onBackgroundPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.target !== e.currentTarget) return;
    drag.current = { kind: "pan", start: { x: e.clientX, y: e.clientY }, origin: pan };
    e.currentTarget.setPointerCapture(e.pointerId);
    setSelected(null);
  };
  const onCardHeaderPointerDown = (id: string) => (e: React.PointerEvent<HTMLDivElement>) => {
    if ((e.target as HTMLElement).closest("button")) return;
    const card = cards.find((c) => c.id === id);
    if (!card) return;
    drag.current = { kind: "card", id, start: { x: e.clientX, y: e.clientY }, origin: { x: card.x, y: card.y } };
    (e.currentTarget.closest("[data-canvas]") as HTMLElement | null)?.setPointerCapture(e.pointerId);
    setSelected(id);
  };
  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    const dx = e.clientX - d.start.x;
    const dy = e.clientY - d.start.y;
    if (d.kind === "pan") setPan({ x: d.origin.x + dx, y: d.origin.y + dy });
    else setCards((cs) => cs.map((c) => (c.id === d.id ? { ...c, x: d.origin.x + dx / zoom, y: d.origin.y + dy / zoom } : c)));
  };
  const onPointerUp = () => (drag.current = null);

  const openView = (card: CardPlacement) => {
    const tool = getTool(getInstance(card.instanceId).type);
    const next = tool.views.find((v) => v.id !== card.viewId) ?? tool.views[0]!;
    const id = `c${Date.now()}`;
    setCards((cs) => [...cs, { id, instanceId: card.instanceId, viewId: next.id, x: card.x + 40, y: card.y + 40, primary: false }]);
    setSelected(id);
  };
  const closeCard = (id: string) => setCards((cs) => cs.filter((c) => c.id !== id));

  const openViewsByInstance = React.useMemo(() => {
    const m = new Map<string, number>();
    for (const c of cards) m.set(c.instanceId, (m.get(c.instanceId) ?? 0) + 1);
    return m;
  }, [cards]);

  const connectedPorts = React.useMemo(() => {
    const m = new Map<string, Set<string>>();
    for (const k of initialConnections) {
      m.set(k.from.instanceId, (m.get(k.from.instanceId) ?? new Set()).add(k.from.portId));
      m.set(k.to.instanceId, (m.get(k.to.instanceId) ?? new Set()).add(k.to.portId));
    }
    return m;
  }, []);

  return (
    <div
      ref={rootRef}
      data-canvas
      className="canvas-grid relative h-full w-full cursor-grab overflow-hidden active:cursor-grabbing"
      style={{ "--grid-x": `${pan.x}px`, "--grid-y": `${pan.y}px`, "--grid-size": `${24 * zoom}px` } as React.CSSProperties}
      onPointerDown={onBackgroundPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
    >
      <div className="absolute top-0 left-0 origin-top-left" style={{ transform: `translate(${pan.x}px, ${pan.y}px) scale(${zoom})` }}>
        <ConnectionLayer cards={cards} connections={initialConnections} />
        {cards.map((card) => {
          const instance = getInstance(card.instanceId);
          const tool = getTool(instance.type);
          const view = tool.views.find((v) => v.id === card.viewId) ?? tool.views[0]!;
          const View = view.component;
          return (
            <div key={card.id} data-card-x={card.x} data-card-y={card.y} className="absolute" style={{ left: card.x, top: card.y, zIndex: selected === card.id ? 2 : 1 }} onPointerDown={() => setSelected(card.id)}>
              <ToolFrame
                instanceName={instance.name}
                typeName={tool.name}
                accent={tool.accent}
                views={tool.views}
                activeView={view.id}
                onViewChange={(id) => setCards((cs) => cs.map((c) => (c.id === card.id ? { ...c, viewId: id } : c)))}
                onOpenView={() => openView(card)}
                onClose={() => closeCard(card.id)}
                openViews={openViewsByInstance.get(card.instanceId)}
                inputs={card.primary ? tool.inputs : []}
                outputs={card.primary ? tool.outputs : []}
                connectedPorts={connectedPorts.get(card.instanceId)}
                width={view.width}
                selected={selected === card.id}
                onHeaderPointerDown={onCardHeaderPointerDown(card.id)}
                className="cursor-default"
              >
                <View instanceId={instance.id} instanceName={instance.name} accent={tool.accent} />
              </ToolFrame>
            </div>
          );
        })}
      </div>

      {/* zoom cluster */}
      <div className="absolute top-3 right-3 flex h-8 items-center gap-0.5 rounded-lg border border-alpha/10 bg-gray-200/95 p-0.5 shadow-card backdrop-blur-md">
        <Tooltip content="Zoom out" shortcut="mod+-" side="bottom">
          <IconButton label="Zoom out" size="sm" variant="ghost-muted" disabled={zoom <= ZOOM_STEPS[0]!} onClick={() => zoomBy(-1)}>
            <Minus />
          </IconButton>
        </Tooltip>
        <Tooltip content="Reset zoom" shortcut="mod+0" side="bottom">
          <Button variant="ghost" size="sm" className="tabular w-12 text-gray-900 text-xs" aria-label={`Zoom ${Math.round(zoom * 100)}%, reset`} onClick={() => setZoom(1)}>
            {Math.round(zoom * 100)}%
          </Button>
        </Tooltip>
        <Tooltip content="Zoom in" shortcut="mod+=" side="bottom">
          <IconButton label="Zoom in" size="sm" variant="ghost-muted" disabled={zoom >= ZOOM_STEPS[ZOOM_STEPS.length - 1]!} onClick={() => zoomBy(1)}>
            <Plus />
          </IconButton>
        </Tooltip>
        <Separator orientation="vertical" className="mx-0.5 h-4" />
        <Tooltip content="Fit all cards" shortcut="shift+1" side="bottom">
          <IconButton label="Fit all cards" size="sm" variant="ghost-muted" onClick={fit}>
            <Maximize2 />
          </IconButton>
        </Tooltip>
      </div>
    </div>
  );
}

const strokeFor: Record<PortKind, { width: number; dash?: string }> = {
  audio: { width: 2.5 },
  event: { width: 1.5, dash: "6 4" },
  mod: { width: 1.5, dash: "1.5 4" },
};

function ConnectionLayer({ cards, connections }: { cards: CardPlacement[]; connections: Connection[] }) {
  const primaryFor = (instanceId: string) => cards.find((c) => c.instanceId === instanceId && c.primary);

  const portPoint = (instanceId: string, portId: string, side: "in" | "out"): Point | null => {
    const card = primaryFor(instanceId);
    if (!card) return null;
    const tool = getTool(getInstance(instanceId).type);
    const view = tool.views.find((v) => v.id === card.viewId) ?? tool.views[0]!;
    const list = side === "in" ? tool.inputs : tool.outputs;
    const i = list.findIndex((p) => p.id === portId);
    if (i < 0) return null;
    return { x: side === "in" ? card.x : card.x + view.width, y: card.y + portOffsetY(i) };
  };

  // Dotted "same instance" links between cards that show the same instance.
  const viewLinks: { key: string; a: Point; b: Point; accent: string }[] = [];
  const byInstance = new Map<string, CardPlacement[]>();
  for (const c of cards) byInstance.set(c.instanceId, [...(byInstance.get(c.instanceId) ?? []), c]);
  for (const [instanceId, list] of byInstance) {
    if (list.length < 2) continue;
    const accent = accents[getTool(getInstance(instanceId).type).accent];
    for (let i = 1; i < list.length; i++) {
      const a = list[0]!;
      const b = list[i]!;
      viewLinks.push({ key: `${a.id}-${b.id}`, a: { x: a.x + 16, y: a.y + 18 }, b: { x: b.x + 16, y: b.y + 18 }, accent });
    }
  }

  return (
    <svg className="pointer-events-none absolute top-0 left-0 overflow-visible" width={1} height={1} aria-hidden>
      {viewLinks.map((l) => (
        <path
          key={l.key}
          d={viewLinkPath(l.a, l.b)}
          fill="none"
          stroke={l.accent}
          strokeOpacity={0.55}
          strokeWidth={1.5}
          strokeDasharray="2 4"
          strokeLinecap="round"
        />
      ))}
      {connections.map((k) => {
        const a = portPoint(k.from.instanceId, k.from.portId, "out");
        const b = portPoint(k.to.instanceId, k.to.portId, "in");
        if (!a || !b) return null;
        const accent = accents[getTool(getInstance(k.from.instanceId).type).accent];
        const s = strokeFor[k.kind];
        const d = wirePath(a, b);
        return (
          <g key={k.id}>
            <path d={d} fill="none" stroke="var(--color-gray-100)" strokeOpacity={0.9} strokeWidth={s.width + 3} strokeLinecap="round" />
            <path d={d} fill="none" stroke={accent} strokeOpacity={0.9} strokeWidth={s.width} strokeDasharray={s.dash} strokeLinecap="round" />
          </g>
        );
      })}
    </svg>
  );
}

/** Soft S-curve between an output (left point) and an input. Handles stay horizontal
   so the wire leaves and enters ports straight, even when the target is behind. */
export function wirePath(a: Point, b: Point) {
  const dx = b.x - a.x;
  const dy = Math.abs(b.y - a.y);
  const handle = Math.min(160, Math.max(Math.abs(dx) * 0.5, Math.min(48, 16 + dy * 0.25)));
  const c1 = { x: a.x + handle, y: a.y };
  const c2 = { x: b.x - handle, y: b.y };
  return `M ${a.x} ${a.y} C ${c1.x} ${c1.y}, ${c2.x} ${c2.y}, ${b.x} ${b.y}`;
}

function viewLinkPath(a: Point, b: Point) {
  const dy = b.y - a.y;
  const bulge = Math.max(20, Math.min(36, Math.abs(dy) * 0.2));
  return `M ${a.x} ${a.y} C ${a.x - bulge} ${a.y + dy * 0.25}, ${b.x - bulge} ${b.y - dy * 0.25}, ${b.x} ${b.y}`;
}

export const kindLegend: { kind: PortKind; label: string }[] = [
  { kind: "audio", label: "Audio" },
  { kind: "event", label: "Events" },
  { kind: "mod", label: "Modulation" },
];

export function WireLegend({ className }: { className?: string }) {
  return (
    <div className={cn("flex h-8 items-center gap-3 rounded-lg border border-alpha/10 bg-gray-200/95 px-2.5 text-2xs text-gray-800 shadow-card backdrop-blur-md", className)} aria-label="Connection kinds">
      {kindLegend.map(({ kind, label }) => {
        const s = strokeFor[kind];
        return (
          <span key={kind} className="flex items-center gap-1.5">
            <svg width={24} height={8} aria-hidden>
              <line x1={1} y1={4} x2={23} y2={4} stroke="var(--color-gray-800)" strokeWidth={s.width} strokeDasharray={s.dash} strokeLinecap="round" />
            </svg>
            {label}
          </span>
        );
      })}
    </div>
  );
}
