import * as React from "react";

/**
 * Shared per-instance state. Two views of the same instance read and write the same
 * values, which is how "same instance in two panes" stays honest in the prototype.
 * Keyed by instance id + state key.
 */
const stores = new Map<string, { value: unknown; listeners: Set<() => void> }>();

function store(key: string, initial: unknown) {
  let s = stores.get(key);
  if (!s) {
    s = { value: initial, listeners: new Set() };
    stores.set(key, s);
  }
  return s;
}

export function useInstanceState<T>(
  instanceId: string,
  key: string,
  initial: T,
): [T, (next: T | ((prev: T) => T)) => void] {
  const fullKey = `${instanceId}:${key}`;
  const s = store(fullKey, initial);
  const subscribe = React.useCallback(
    (cb: () => void) => {
      s.listeners.add(cb);
      return () => s.listeners.delete(cb);
    },
    [s],
  );
  const value = React.useSyncExternalStore(subscribe, () => s.value as T);
  const set = React.useCallback(
    (next: T | ((prev: T) => T)) => {
      const resolved = typeof next === "function" ? (next as (p: T) => T)(s.value as T) : next;
      if (Object.is(resolved, s.value)) return;
      s.value = resolved;
      for (const l of s.listeners) l();
    },
    [s],
  );
  return [value, set];
}
