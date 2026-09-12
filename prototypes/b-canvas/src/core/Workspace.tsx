import * as React from "react";
import { SegmentedControl } from "@/ui";
import { AgentSidebar } from "./AgentSidebar";
import { Canvas, WireLegend } from "./Canvas";
import { buildStatusFor, conversationFor } from "./conversation";
import { type DevState, useDevState } from "./dev-state";
import { ProjectCluster } from "./ProjectCluster";
import { TransportPill } from "./TransportPill";

/** The whole window: agent sidebar on the left, canvas with floating chrome on the right. */
export function Workspace() {
  const [state, setState] = useDevState();
  const [collapsed, setCollapsed] = React.useState(false);
  const messages = React.useMemo(() => conversationFor(state), [state]);
  const build = React.useMemo(() => buildStatusFor(state), [state]);

  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey && e.key === "j") {
        e.preventDefault();
        setCollapsed((c) => !c);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex h-full w-full overflow-hidden bg-gray-50 text-gray-950">
      <AgentSidebar
        messages={messages}
        state={state}
        collapsed={collapsed}
        onToggleCollapsed={() => setCollapsed((c) => !c)}
        footer={<DevToggle state={state} onChange={setState} />}
      />
      <main className="relative min-w-0 flex-1">
        <Canvas />
        {/* window drag strip over the canvas top edge */}
        <div className="app-drag-region pointer-events-none absolute inset-x-0 top-0 h-7" aria-hidden />
        <div className="absolute top-3 left-3">
          <ProjectCluster />
        </div>
        <div className="absolute top-3 right-[184px]">
          <WireLegend />
        </div>
        <div className="absolute bottom-4 left-1/2 -translate-x-1/2">
          <TransportPill build={build} />
        </div>
      </main>
    </div>
  );
}

function DevToggle({ state, onChange }: { state: DevState; onChange: (s: DevState) => void }) {
  return (
    <div className="flex h-7 items-center gap-2">
      <span className="font-mono text-2xs text-gray-700">prototype</span>
      <SegmentedControl
        size="xs"
        label="Prototype state"
        value={state}
        onValueChange={onChange}
        options={[
          { value: "default", label: "Default" },
          { value: "building", label: "Building" },
          { value: "failed", label: "Failed" },
        ]}
      />
      <span className="flex-1" />
      <a href="#/gallery" className="focus-ring rounded-md px-1.5 py-0.5 text-2xs text-gray-800 hover:text-gray-950">
        Primitives
      </a>
    </div>
  );
}
