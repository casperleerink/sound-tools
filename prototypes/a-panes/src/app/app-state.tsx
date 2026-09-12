import * as React from "react";
import type { BuildState } from "@/data/conversation";

export type PaneTab = { id: string; instanceId: string; viewId: string };
export type PaneLeaf = { kind: "pane"; id: string; tabs: PaneTab[]; active: string };
export type PaneSplit = { kind: "split"; id: string; direction: "row" | "column"; children: PaneNode[]; sizes: number[] };
export type PaneNode = PaneLeaf | PaneSplit;

export type Transport = { state: "playing" | "paused" | "stopped"; position: number };

interface AppState {
  buildState: BuildState;
  setBuildState: (s: BuildState) => void;
  layout: PaneNode;
  setLayout: React.Dispatch<React.SetStateAction<PaneNode>>;
  focusedPane: string;
  setFocusedPane: (id: string) => void;
  transport: Transport;
  setTransport: React.Dispatch<React.SetStateAction<Transport>>;
  sidebarOpen: boolean;
  setSidebarOpen: (v: boolean) => void;
  railOpen: boolean;
  setRailOpen: (v: boolean) => void;
}

const Ctx = React.createContext<AppState | null>(null);

const DEFAULT_LAYOUT: PaneNode = {
  kind: "split",
  id: "root",
  direction: "row",
  sizes: [0.6, 0.4],
  children: [
    {
      kind: "split",
      id: "left",
      direction: "column",
      sizes: [0.56, 0.44],
      children: [
        {
          kind: "pane",
          id: "p1",
          active: "t1",
          tabs: [
            { id: "t1", instanceId: "loops", viewId: "patterns" },
            { id: "t2", instanceId: "tone-a", viewId: "main" },
            { id: "t3", instanceId: "tremolo", viewId: "main" },
          ],
        },
        {
          kind: "pane",
          id: "p2",
          active: "t4",
          tabs: [
            { id: "t4", instanceId: "lattice", viewId: "main" },
            { id: "t5", instanceId: "tone-c", viewId: "main" },
          ],
        },
      ],
    },
    {
      kind: "split",
      id: "right",
      direction: "column",
      sizes: [0.5, 0.5],
      children: [
        {
          kind: "pane",
          id: "p3",
          active: "t6",
          tabs: [
            { id: "t6", instanceId: "loops", viewId: "voices" },
            { id: "t7", instanceId: "tone-b", viewId: "main" },
          ],
        },
        { kind: "pane", id: "p4", active: "t8", tabs: [{ id: "t8", instanceId: "mix", viewId: "main" }] },
      ],
    },
  ],
};

function initialBuildState(): BuildState {
  const s = new URLSearchParams(window.location.search).get("state");
  return s === "building" || s === "failed" ? s : "idle";
}

export function AppStateProvider({ children }: { children: React.ReactNode }) {
  const [buildState, setBuildStateRaw] = React.useState<BuildState>(initialBuildState);
  const setBuildState = (s: BuildState) => {
    setBuildStateRaw(s);
    const url = new URL(window.location.href);
    if (s === "idle") url.searchParams.delete("state");
    else url.searchParams.set("state", s);
    window.history.replaceState(null, "", url);
  };
  const [layout, setLayout] = React.useState<PaneNode>(DEFAULT_LAYOUT);
  const [focusedPane, setFocusedPane] = React.useState("p1");
  const [transport, setTransport] = React.useState<Transport>({ state: "playing", position: 84.35 });
  const [sidebarOpen, setSidebarOpen] = React.useState(true);
  const [railOpen, setRailOpen] = React.useState(() => window.innerWidth >= 1300);

  // Fake position ticking while playing.
  React.useEffect(() => {
    if (transport.state !== "playing") return;
    const id = window.setInterval(() => setTransport((t) => ({ ...t, position: t.position + 0.1 })), 100);
    return () => window.clearInterval(id);
  }, [transport.state]);

  return (
    <Ctx.Provider
      value={{
        buildState,
        setBuildState,
        layout,
        setLayout,
        focusedPane,
        setFocusedPane,
        transport,
        setTransport,
        sidebarOpen,
        setSidebarOpen,
        railOpen,
        setRailOpen,
      }}
    >
      {children}
    </Ctx.Provider>
  );
}

export function useAppState() {
  const v = React.useContext(Ctx);
  if (!v) throw new Error("useAppState outside provider");
  return v;
}

/* Layout helpers */

export function leaves(node: PaneNode): PaneLeaf[] {
  return node.kind === "pane" ? [node] : node.children.flatMap(leaves);
}

export function mapLeaves(node: PaneNode, fn: (leaf: PaneLeaf) => PaneNode): PaneNode {
  if (node.kind === "pane") return fn(node);
  return { ...node, children: node.children.map((c) => mapLeaves(c, fn)) };
}

/** Remove panes with no tabs and collapse splits with one child. */
export function prune(node: PaneNode): PaneNode | null {
  if (node.kind === "pane") return node.tabs.length ? node : null;
  const kept: PaneNode[] = [];
  const sizes: number[] = [];
  node.children.forEach((c, i) => {
    const p = prune(c);
    if (p) {
      kept.push(p);
      sizes.push(node.sizes[i] ?? 1);
    }
  });
  if (kept.length === 0) return null;
  if (kept.length === 1) return kept[0] ?? null;
  const total = sizes.reduce((a, b) => a + b, 0);
  return { ...node, children: kept, sizes: sizes.map((s) => s / total) };
}

export function openViewCount(node: PaneNode, instanceId: string) {
  return leaves(node).reduce((n, leaf) => n + leaf.tabs.filter((t) => t.instanceId === instanceId).length, 0);
}
