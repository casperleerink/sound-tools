import * as React from "react";
import { getTool } from "@/extensions";
import { accents } from "@/lib/accent";
import { ToolFrame, portOffsetY } from "@/ui";
import type { PortKind } from "./sdk";
import { type CardPlacement, type Connection, connections, getInstance, initialCards } from "./project";

const ZOOM_MIN = 0.5;
const ZOOM_MAX = 1.5;
/* Room the project menu and the transport need when fitting, plus side gutters. */
const FIT_INSET = { top: 64, right: 40, bottom: 96, left: 40 };

interface Point {
  x: number;
  y: number;
}

/* Pannable workspace. Cards are absolutely positioned in canvas space; an SVG
   layer in the same space draws the wires. Fits on load; zoom with the trackpad
   (pinch) or the keyboard (mod + / - / 0). */
export function Canvas() {
  const [cards, setCards] = React.useState<CardPlacement[]>(initialCards);
  const [pan, setPan] = React.useState<Point>({ x: 0, y: 0 });
  const [zoom, setZoom] = React.useState(1);
  const [selected, setSelected] = React.useState<string | null>(null);
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
    const z = Math.max(ZOOM_MIN, Math.round(raw * 20) / 20);
    setZoom(z);
    setPan({
      x: FIT_INSET.left + (availW - (maxX - minX) * z) / 2 - minX * z,
      y: FIT_INSET.top + (availH - (maxY - minY) * z) / 2 - minY * z,
    });
  }, []);
  React.useLayoutEffect(fit, [fit]);

  /** Zoom around a screen point so what is under the cursor stays put. */
  const zoomAt = React.useCallback((factor: number, at?: Point) => {
    const root = rootRef.current;
    if (!root) return;
    const r = root.getBoundingClientRect();
    const p = at ? { x: at.x - r.left, y: at.y - r.top } : { x: r.width / 2, y: r.height / 2 };
    setZoom((z) => {
      const next = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, z * factor));
      setPan((pan) => ({ x: p.x - ((p.x - pan.x) * next) / z, y: p.y - ((p.y - pan.y) * next) / z }));
      return next;
    });
  }, []);

  React.useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      if (e.ctrlKey || e.metaKey) zoomAt(Math.exp(-e.deltaY * 0.01), { x: e.clientX, y: e.clientY });
      else setPan((p) => ({ x: p.x - e.deltaX, y: p.y - e.deltaY }));
    };
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey || (e.target as HTMLElement).closest("input, textarea")) return;
      if (e.key === "=" || e.key === "+") zoomAt(1.1);
      else if (e.key === "-") zoomAt(1 / 1.1);
      else if (e.key === "0") fit();
      else return;
      e.preventDefault();
    };
    root.addEventListener("wheel", onWheel, { passive: false });
    window.addEventListener("keydown", onKey);
    return () => {
      root.removeEventListener("wheel", onWheel);
      window.removeEventListener("keydown", onKey);
    };
  }, [fit, zoomAt]);

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

  const closeCard = (id: string) => setCards((cs) => cs.filter((c) => c.id !== id));

  const connectedPorts = React.useMemo(() => {
    const m = new Map<string, Set<string>>();
    for (const k of connections) {
      m.set(k.from.instanceId, (m.get(k.from.instanceId) ?? new Set()).add(k.from.portId));
      m.set(k.to.instanceId, (m.get(k.to.instanceId) ?? new Set()).add(k.to.portId));
    }
    return m;
  }, []);

  return (
    <div
      ref={rootRef}
      data-canvas
      className="relative h-full w-full cursor-grab overflow-hidden bg-gray-100 active:cursor-grabbing"
      onPointerDown={onBackgroundPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
    >
      <div className="absolute top-0 left-0 origin-top-left" style={{ transform: `translate(${pan.x}px, ${pan.y}px) scale(${zoom})` }}>
        <WireLayer cards={cards} connections={connections} />
        {cards.map((card) => {
          const instance = getInstance(card.instanceId);
          const tool = getTool(instance.type);
          const view = tool.views.find((v) => v.id === card.viewId) ?? tool.views[0]!;
          const View = view.component;
          return (
            <div
              key={card.id}
              data-card-x={card.x}
              data-card-y={card.y}
              className="absolute"
              style={{ left: card.x, top: card.y, zIndex: selected === card.id ? 2 : 1 }}
              onPointerDown={() => setSelected(card.id)}
            >
              <ToolFrame
                instanceName={instance.name}
                typeName={tool.name}
                accent={tool.accent}
                views={tool.views}
                activeView={view.id}
                onViewChange={(id) => setCards((cs) => cs.map((c) => (c.id === card.id ? { ...c, viewId: id } : c)))}
                onClose={() => closeCard(card.id)}
                inputs={tool.inputs}
                outputs={tool.outputs}
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
    </div>
  );
}

/* One weight for every wire; the kind shows in the opacity only. */
export const wireOpacity: Record<PortKind, number> = { audio: 0.9, event: 0.55, mod: 0.35 };

function WireLayer({ cards, connections }: { cards: CardPlacement[]; connections: Connection[] }) {
  const portPoint = (instanceId: string, portId: string, side: "in" | "out"): Point | null => {
    const card = cards.find((c) => c.instanceId === instanceId);
    if (!card) return null;
    const tool = getTool(getInstance(instanceId).type);
    const view = tool.views.find((v) => v.id === card.viewId) ?? tool.views[0]!;
    const list = side === "in" ? tool.inputs : tool.outputs;
    const i = list.findIndex((p) => p.id === portId);
    if (i < 0) return null;
    return { x: side === "in" ? card.x : card.x + view.width, y: card.y + portOffsetY(i) };
  };

  return (
    <svg className="pointer-events-none absolute top-0 left-0 overflow-visible" width={1} height={1} aria-hidden>
      {connections.map((k) => {
        const a = portPoint(k.from.instanceId, k.from.portId, "out");
        const b = portPoint(k.to.instanceId, k.to.portId, "in");
        if (!a || !b) return null;
        const accent = accents[getTool(getInstance(k.from.instanceId).type).accent];
        return <path key={k.id} d={wirePath(a, b)} fill="none" stroke={accent} strokeOpacity={wireOpacity[k.kind]} strokeWidth={1.5} strokeLinecap="round" />;
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
