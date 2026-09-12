/* Which agent/build story the window shows. Switch with ?state=building|failed. */
export type DevState = "default" | "building" | "failed";

const STATES: DevState[] = ["default", "building", "failed"];

export function readDevState(): DevState {
  const s = new URLSearchParams(window.location.search).get("state");
  return STATES.includes(s as DevState) ? (s as DevState) : "default";
}
