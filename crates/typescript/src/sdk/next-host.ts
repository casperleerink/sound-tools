// What the runtime runs a project's tools with. Not for tools: they import `./sdk`.

import { registry } from "./tools";
export type { Seen, ToolView, ViewContext } from "./view";

/** Makes `target` hold what `value` holds, so code that kept `target` reads the new values. */
function reconcile(target: Record<PropertyKey, unknown>, value: Record<PropertyKey, unknown>) {
  if (Array.isArray(target) && Array.isArray(value)) {
    target.splice(0, target.length, ...value);
    return;
  }
  for (const key of Object.keys(target)) if (!(key in value)) delete target[key];
  Object.assign(target, value);
}

/** What the host calls. Records and memory are plain objects: a view runs again as a whole. */
export const host = Object.assign(registry, {
  store: <T extends object>(target: T, _writable: boolean): T => target,
  adopted: <T>(value: T): T => value,
  reconcile,
  batch: <T>(change: () => T): T => change(),
  readFields: (_target: object): string[] | null => [],
  isRead: (_target: object): boolean => false,
  /** Called once bindings ran again; the host sends their ops. */
  changed: () => {},
  binding: () => false,
});
