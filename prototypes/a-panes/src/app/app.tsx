import * as React from "react";
import { AgentSidebar } from "@/agent/agent-sidebar";
import type { BuildState } from "@/data/conversation";
import { Gallery } from "@/gallery/gallery";
import { SegmentedControl } from "@/ui";
import { AppStateProvider, useAppState } from "./app-state";
import { LeftRail } from "./left-rail";
import { PaneTree } from "./panes";
import { TransportBar } from "./transport-bar";

function useHashRoute() {
  const [hash, setHash] = React.useState(window.location.hash);
  React.useEffect(() => {
    const on = () => setHash(window.location.hash);
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);
  return hash;
}

export function App() {
  const hash = useHashRoute();
  return (
    <AppStateProvider>
      {hash.startsWith("#/gallery") ? <Gallery /> : <Window />}
    </AppStateProvider>
  );
}

function Window() {
  const { layout } = useAppState();
  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-gray-50 text-gray-950">
      <DragRegion />
      <div className="flex min-h-0 flex-1">
        <LeftRail />
        <main className="flex min-h-0 min-w-0 flex-1 p-1.5">
          <PaneTree node={layout} />
        </main>
        <AgentSidebar />
      </div>
      <TransportBar />
    </div>
  );
}

/** Frameless window: 28px drag strip with the macOS traffic-light area kept clear. */
function DragRegion() {
  const { buildState, setBuildState } = useAppState();
  return (
    <div className="app-drag-region relative flex h-7 shrink-0 items-center bg-gray-50 pl-[76px] pr-2">
      <TrafficLights />
      <span className="text-xs text-gray-950/30">Sound Tools</span>
      <div className="app-no-drag ml-auto flex items-center gap-2">
        <span className="text-2xs text-gray-950/30">dev</span>
        <SegmentedControl<BuildState>
          aria-label="Build state (dev)"
          size="xs"
          value={buildState}
          onValueChange={setBuildState}
          options={[
            { value: "idle", label: "idle" },
            { value: "building", label: "building" },
            { value: "failed", label: "failed" },
          ]}
        />
        <a href="#/gallery" className="rounded px-1 text-2xs text-gray-950/40 hover:text-gray-950">
          primitives
        </a>
      </div>
    </div>
  );
}

function TrafficLights() {
  return (
    <div aria-hidden className="absolute left-3 top-1/2 flex -translate-y-1/2 gap-2">
      <span className="size-3 rounded-full bg-[#ff5f57]" />
      <span className="size-3 rounded-full bg-[#febc2e]" />
      <span className="size-3 rounded-full bg-[#28c840]" />
    </div>
  );
}
