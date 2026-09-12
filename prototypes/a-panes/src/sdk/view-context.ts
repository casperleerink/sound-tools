import * as React from "react";
import type { Port, ToolInstance } from "./types";

export interface ResolvedConnection {
  kind: Port["kind"];
  /** Port label on this instance. */
  port: string;
  /** Other instance name. */
  peer: string;
  peerPort: string;
}

/** What the core knows about the view being rendered. Read by ToolView, not by extensions. */
export interface ViewContextValue {
  instance: ToolInstance;
  viewId: string;
  /** How many views of this instance are open in the workspace, this one included. */
  openViews: number;
  inputs: ResolvedConnection[];
  outputs: ResolvedConnection[];
  /** Extension currently being rebuilt, if any. */
  rebuilding?: { extension: string; failed: boolean };
}

export const ViewContext = React.createContext<ViewContextValue | null>(null);

export function useViewContext() {
  return React.useContext(ViewContext);
}
