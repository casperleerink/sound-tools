import * as React from "react";

/* Which agent/build story the window shows. Switch with ?state=building|failed
   or the dev toggle in the bottom-right corner. */
export type DevState = "default" | "building" | "failed";

const STATES: DevState[] = ["default", "building", "failed"];

function read(): DevState {
  const s = new URLSearchParams(window.location.search).get("state");
  return STATES.includes(s as DevState) ? (s as DevState) : "default";
}

export function useDevState(): [DevState, (s: DevState) => void] {
  const [state, setState] = React.useState<DevState>(read);
  const set = React.useCallback((s: DevState) => {
    const url = new URL(window.location.href);
    if (s === "default") url.searchParams.delete("state");
    else url.searchParams.set("state", s);
    window.history.replaceState(null, "", url);
    setState(s);
  }, []);
  return [state, set];
}
