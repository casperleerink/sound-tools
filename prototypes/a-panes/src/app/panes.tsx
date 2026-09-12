import { Columns2, Plus, Rows2, X } from "lucide-react";
import * as React from "react";
import { connectionsFor, instanceById, INSTANCES } from "@/data/project";
import { toolFor } from "@/extensions";
import { ACCENT_VAR } from "@/sdk/types";
import { ViewContext, type ResolvedConnection } from "@/sdk/view-context";
import { cn, Dot, IconButton, Tooltip } from "@/ui";
import { leaves, mapLeaves, openViewCount, prune, useAppState, type PaneLeaf, type PaneNode, type PaneTab } from "./app-state";

let nextId = 100;
const uid = (p: string) => `${p}${nextId++}`;

export function PaneTree({ node }: { node: PaneNode }) {
  if (node.kind === "pane") return <Pane leaf={node} />;
  return (
    <div className={cn("flex min-h-0 min-w-0 flex-1 gap-1.5", node.direction === "row" ? "flex-row" : "flex-col")}>
      {node.children.map((child, i) => (
        <div key={child.id} className="flex min-h-0 min-w-0" style={{ flex: `${node.sizes[i] ?? 1} 1 0%` }}>
          <PaneTree node={child} />
        </div>
      ))}
    </div>
  );
}

function resolve(conns: ReturnType<typeof connectionsFor>, self: string) {
  const name = (id: string) => instanceById(id).name;
  const label = (p: string) => p.replace(/-/g, " ");
  const inputs: ResolvedConnection[] = conns.inputs.map((c) => ({ kind: c.kind, port: label(c.to.port), peer: name(c.from.instance), peerPort: label(c.from.port) }));
  const outputs: ResolvedConnection[] = conns.outputs.map((c) => ({ kind: c.kind, port: label(c.from.port), peer: name(c.to.instance), peerPort: label(c.to.port) }));
  void self;
  return { inputs, outputs };
}

function Pane({ leaf }: { leaf: PaneLeaf }) {
  const { layout, setLayout, focusedPane, setFocusedPane, buildState } = useAppState();
  const [dragOver, setDragOver] = React.useState(false);
  const [addOpen, setAddOpen] = React.useState(false);
  const focused = focusedPane === leaf.id;
  const active = leaf.tabs.find((t) => t.id === leaf.active) ?? leaf.tabs[0];

  const update = (fn: (l: PaneLeaf) => PaneLeaf) =>
    setLayout((root) => prune(mapLeaves(root, (l) => (l.id === leaf.id ? fn(l) : l))) ?? root);

  const closeTab = (tabId: string) =>
    update((l) => {
      const tabs = l.tabs.filter((t) => t.id !== tabId);
      const idx = l.tabs.findIndex((t) => t.id === tabId);
      const next = l.active === tabId ? (tabs[Math.max(0, idx - 1)]?.id ?? tabs[0]?.id ?? "") : l.active;
      return { ...l, tabs, active: next };
    });

  const split = (direction: "row" | "column") => {
    if (!active) return;
    const copy: PaneTab = { id: uid("t"), instanceId: active.instanceId, viewId: active.viewId };
    setLayout((root) =>
      mapLeaves(root, (l) =>
        l.id === leaf.id
          ? { kind: "split", id: uid("s"), direction, sizes: [0.5, 0.5], children: [l, { kind: "pane", id: uid("p"), tabs: [copy], active: copy.id }] }
          : l,
      ),
    );
  };

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(false);
    const raw = e.dataTransfer.getData("application/x-sound-tools-tab");
    if (!raw) return;
    const { tabId, fromPane } = JSON.parse(raw) as { tabId: string; fromPane: string };
    if (fromPane === leaf.id) return;
    let moved: PaneTab | undefined;
    for (const l of leaves(layout)) if (l.id === fromPane) moved = l.tabs.find((t) => t.id === tabId);
    if (!moved) return;
    const tab = moved;
    setLayout((root) => {
      const next = mapLeaves(root, (l) => {
        if (l.id === fromPane) {
          const tabs = l.tabs.filter((t) => t.id !== tabId);
          return { ...l, tabs, active: l.active === tabId ? (tabs[0]?.id ?? "") : l.active };
        }
        if (l.id === leaf.id) return { ...l, tabs: [...l.tabs, tab], active: tab.id };
        return l;
      });
      return prune(next) ?? root;
    });
    setFocusedPane(leaf.id);
  };

  const addView = (instanceId: string, viewId: string) => {
    const tab: PaneTab = { id: uid("t"), instanceId, viewId };
    update((l) => ({ ...l, tabs: [...l.tabs, tab], active: tab.id }));
    setAddOpen(false);
  };

  const rebuilding = buildState === "idle" ? undefined : { extension: "rhythm-loops", failed: buildState === "failed" };

  return (
    <div
      className={cn("group/pane flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-lg bg-gray-200", focused ? "ring-1 ring-alpha/10" : "ring-1 ring-transparent")}
      onPointerDownCapture={() => setFocusedPane(leaf.id)}
      onDragOver={(e) => {
        if (e.dataTransfer.types.includes("application/x-sound-tools-tab")) {
          e.preventDefault();
          setDragOver(true);
        }
      }}
      onDragLeave={() => setDragOver(false)}
      onDrop={onDrop}
    >
      {/* Tab strip */}
      <div className="relative flex h-8 shrink-0 items-stretch bg-gray-100">
        <div role="tablist" className="flex min-w-0 flex-1 items-stretch overflow-x-auto scrollbar-hidden">
          {leaf.tabs.map((tab) => (
            <TabButton
              key={tab.id}
              tab={tab}
              paneId={leaf.id}
              active={tab.id === leaf.active}
              paneFocused={focused}
              onSelect={() => update((l) => ({ ...l, active: tab.id }))}
              onClose={() => closeTab(tab.id)}
            />
          ))}
          <div className="relative flex items-center pl-0.5">
            <IconButton label="Open a view in this pane" size="xs" variant="quiet" onClick={() => setAddOpen((v) => !v)}>
              <Plus />
            </IconButton>
            {addOpen && <AddViewMenu onPick={addView} onClose={() => setAddOpen(false)} />}
          </div>
        </div>
        <div className={cn("flex shrink-0 items-center gap-0.5 pr-1 transition-opacity duration-100 focus-within:opacity-100 group-hover/pane:opacity-100", focused ? "opacity-100" : "opacity-0")}>
          <Tooltip content="Split right">
            <IconButton label="Split right" size="xs" variant="quiet" onClick={() => split("row")}>
              <Columns2 />
            </IconButton>
          </Tooltip>
          <Tooltip content="Split down">
            <IconButton label="Split down" size="xs" variant="quiet" onClick={() => split("column")}>
              <Rows2 />
            </IconButton>
          </Tooltip>
        </div>
      </div>
      {/* View */}
      <div className={cn("relative min-h-0 min-w-0 flex-1", dragOver && "outline-2 -outline-offset-2 outline-lavender-500/60")}>
        {active ? (
          <ViewHost tab={active} openViews={openViewCount(layout, active.instanceId)} rebuilding={rebuilding} />
        ) : (
          <div className="flex h-full items-center justify-center text-sm text-gray-950/40">Empty pane</div>
        )}
      </div>
    </div>
  );
}

function ViewHost({ tab, openViews, rebuilding }: { tab: PaneTab; openViews: number; rebuilding?: { extension: string; failed: boolean } }) {
  const instance = instanceById(tab.instanceId);
  const tool = toolFor(instance);
  const view = tool.views.find((v) => v.id === tab.viewId) ?? tool.views[0];
  if (!view) return null;
  const View = view.component;
  const conns = resolve(connectionsFor(instance.id), instance.id);
  const ctx = { instance, viewId: view.id, openViews, inputs: conns.inputs, outputs: conns.outputs, ...(rebuilding ? { rebuilding } : {}) };
  return (
    <div className="h-full min-h-0" style={{ "--accent": ACCENT_VAR[tool.accent] } as React.CSSProperties}>
      <ViewContext.Provider value={ctx}>
        <View instance={instance} viewId={view.id} />
      </ViewContext.Provider>
    </div>
  );
}

function TabButton({
  tab,
  paneId,
  active,
  paneFocused,
  onSelect,
  onClose,
}: {
  tab: PaneTab;
  paneId: string;
  active: boolean;
  paneFocused: boolean;
  onSelect: () => void;
  onClose: () => void;
}) {
  const instance = instanceById(tab.instanceId);
  const tool = toolFor(instance);
  const view = tool.views.find((v) => v.id === tab.viewId);
  const showView = tool.views.length > 1 && view;
  return (
    <div
      role="tab"
      tabIndex={0}
      aria-selected={active}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData("application/x-sound-tools-tab", JSON.stringify({ tabId: tab.id, fromPane: paneId }));
        e.dataTransfer.effectAllowed = "move";
      }}
      onClick={onSelect}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSelect();
        }
        if (e.key === "Backspace" || e.key === "Delete") onClose();
      }}
      className={cn(
        "group/tab relative flex h-8 min-w-0 max-w-56 shrink-0 cursor-default select-none items-center gap-2 border-r border-alpha/5 pl-3 pr-1.5 text-sm transition-colors duration-100",
        active ? "bg-gray-200 text-gray-950" : "text-gray-950/50 hover:bg-alpha/[0.03] hover:text-gray-950/80",
      )}
    >
      {active && <span aria-hidden className={cn("absolute inset-x-0 top-0 h-px", paneFocused ? "bg-(--accent)" : "bg-alpha/10")} style={{ "--accent": ACCENT_VAR[tool.accent] } as React.CSSProperties} />}
      <Dot color={ACCENT_VAR[tool.accent]} className={cn(!active && "opacity-60")} />
      <span className="truncate font-medium">{instance.name}</span>
      {(showView || tool.name !== instance.name) && (
        <span className={cn("truncate text-xs", active ? "text-gray-950/40" : "text-gray-950/30")}>{showView ? view.label : tool.name}</span>
      )}
      <button
        type="button"
        aria-label={`Close ${instance.name}`}
        onClick={(e) => {
          e.stopPropagation();
          onClose();
        }}
        className={cn(
          "ml-auto flex size-5 shrink-0 items-center justify-center rounded text-gray-950/50 opacity-0 transition-opacity duration-100 hover:bg-alpha/10 hover:text-gray-950 focus-visible:opacity-100 group-hover/tab:opacity-100",
        )}
      >
        <X className="size-3.5" />
      </button>
    </div>
  );
}

function AddViewMenu({ onPick, onClose }: { onPick: (instanceId: string, viewId: string) => void; onClose: () => void }) {
  const ref = React.useRef<HTMLDivElement>(null);
  React.useEffect(() => {
    const onDown = (e: PointerEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("pointerdown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointerdown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [onClose]);
  return (
    <div ref={ref} role="menu" className="absolute left-0 top-full z-40 mt-1 w-60 rounded-lg bg-gray-300 p-1 shadow-popup">
      <div className="px-2 pb-1 pt-1.5 text-2xs font-semibold uppercase tracking-[0.08em] text-gray-950/40">Open view</div>
      {INSTANCES.map((inst) => {
        const tool = toolFor(inst);
        return tool.views.map((v) => (
          <button
            key={`${inst.id}-${v.id}`}
            type="button"
            role="menuitem"
            onClick={() => onPick(inst.id, v.id)}
            className="flex h-7 w-full items-center gap-2 rounded-md px-2 text-sm text-gray-950 hover:bg-alpha/10"
          >
            <Dot color={ACCENT_VAR[tool.accent]} />
            <span className="font-medium">{inst.name}</span>
            {(tool.views.length > 1 || tool.name !== inst.name) && (
              <span className="text-xs text-gray-950/40">{tool.views.length > 1 ? v.label : tool.name}</span>
            )}
          </button>
        ));
      })}
    </div>
  );
}
