import * as React from "react";
import { AgentSidebar } from "./AgentSidebar";
import { Canvas } from "./Canvas";
import { conversationFor } from "./conversation";
import { readDevState } from "./dev-state";
import { ProjectMenu } from "./ProjectMenu";
import { TransportPill } from "./TransportPill";

/** The whole window: agent sidebar on the left, canvas with a project menu and the transport on the right. */
export function Workspace() {
  const state = React.useMemo(readDevState, []);
  const turns = React.useMemo(() => conversationFor(state), [state]);

  return (
    <div className="flex h-full w-full overflow-hidden bg-gray-50 text-gray-950">
      <AgentSidebar turns={turns} />
      <main className="relative min-w-0 flex-1">
        <Canvas />
        {/* window drag strip over the canvas top edge */}
        <div className="app-drag-region pointer-events-none absolute inset-x-0 top-0 h-7" aria-hidden />
        <div className="absolute top-2 left-4">
          <ProjectMenu />
        </div>
        <div className="absolute bottom-6 left-1/2 -translate-x-1/2">
          <TransportPill reloadPending={state === "building"} />
        </div>
      </main>
    </div>
  );
}
