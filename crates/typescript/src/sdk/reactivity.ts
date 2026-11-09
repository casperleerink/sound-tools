// Reactivity. A view and a component run once and make the nodes the window keeps. A
// function given as a prop or a child is a binding: it runs again when a record field, memory,
// an output or a signal it read changes, and only what it gives the window changes there.

import { host } from "./host";
import { CardRoot } from "./nodes";

/** A function the window calls again when what it read changed. */
export type Accessor<T> = () => T;

/**
 * A prop as written: a value, which stays, or an `Accessor`, which runs again when what it
 * reads changes: `x={() => note.start}`. An accessor may give a live value: `x={() =>
 * picked() ? move.dx.plus(note.start) : note.start}`.
 */
export type Bind<T> = T | Accessor<T>;

/** Memory as code gets it: change it by setting a field. A `Set` or a `Map` is replaced, never changed in place. */
export type Memory<M> = M extends Set<infer T>
  ? ReadonlySet<T>
  : M extends Map<infer K, infer V>
    ? ReadonlyMap<K, V>
    : M extends (...args: never[]) => unknown
      ? M
      : M extends object
        ? { -readonly [Key in keyof M]: Memory<M[Key]> }
        : M;

export interface Owner {
  owner: Owner | null;
  /** Made on the first: most bindings own nothing and clean up nothing. */
  owned?: Computation[];
  cleanups?: Array<() => void>;
  /** The card whose nodes it makes. */
  card: CardRoot | null;
  disposed: boolean;
}

interface Computation extends Owner {
  run: () => void;
  sources: Dep[];
  /** Made earlier runs first, so a binding runs after what made it. */
  order: number;
  queued: boolean;
}

interface Dep {
  observers: Set<Computation>;
}

export let listener: Computation | null = null;
export let owner: Owner | null = null;
/** Above 0 while the body of a component runs, which reads nothing that changes. */
export let settingUp = 0;
let batching = 0;
let queue: Computation[] = [];
let order = 0;

const READ_IN_SETUP =
  "a view and a component run once, so a value that changes is read inside a function the window calls again: " +
  "x={() => note.start}, not x={note.start}; {() => `${state.count} notes`} as a child; <For each={() => state.notes}>";

function dep(): Dep {
  return { observers: new Set() };
}

function track(read: Dep) {
  if (listener) {
    if (!read.observers.has(listener)) {
      read.observers.add(listener);
      listener.sources.push(read);
    }
  } else if (settingUp > 0) {
    throw new Error(READ_IN_SETUP);
  }
}

function notify(changed: Dep | undefined) {
  if (!changed) return;
  for (const computation of changed.observers) {
    if (!computation.queued) {
      computation.queued = true;
      queue.push(computation);
    }
  }
  if (batching === 0) flush();
}

/** Runs `change` and then, once, every binding what it changed reaches. */
export function batch<T>(change: () => T): T {
  batching += 1;
  try {
    return change();
  } finally {
    batching -= 1;
    if (batching === 0) flush();
  }
}

function flush() {
  if (queue.length === 0) return;
  batching += 1;
  try {
    while (queue.length > 0) {
      const due = queue.sort((a, b) => a.order - b.order);
      queue = [];
      for (const computation of due) {
        computation.queued = false;
        if (!computation.disposed) rerun(computation);
      }
    }
  } finally {
    batching -= 1;
  }
  // Bindings ran again: their ops go out, also when code changed memory after an `await`.
  host.changed();
}

function clean(node: Owner) {
  for (const child of node.owned ?? []) dispose(child);
  node.owned = undefined;
  const cleanups = node.cleanups;
  node.cleanups = undefined;
  for (const cleanup of cleanups ?? []) cleanup();
  if ("sources" in node) {
    const computation = node as Computation;
    for (const read of computation.sources) read.observers.delete(computation);
    computation.sources = [];
  }
}

export function dispose(node: Owner) {
  clean(node);
  node.disposed = true;
}

function rerun(computation: Computation) {
  clean(computation);
  const [lastListener, lastOwner, lastSetup] = [listener, owner, settingUp];
  [listener, owner, settingUp] = [computation, computation, 0];
  try {
    computation.run();
  } catch (error) {
    failedIn(computation, error);
  } finally {
    [listener, owner, settingUp] = [lastListener, lastOwner, lastSetup];
  }
}

function failedIn(where: Owner, error: unknown) {
  let card: CardRoot | null = null;
  for (let at: Owner | null = where; at && !card; at = at.owner) card = at.card;
  if (!card) throw error;
  card.error ??= String(error);
}

function computation(run: () => void): Computation {
  const made: Computation = { run, sources: [], owner, card: owner?.card ?? null, disposed: false, order: order++, queued: false };
  if (owner) (owner.owned ??= []).push(made);
  return made;
}

/** Runs `run` now and again whenever what it read changes. */
export function effect(run: () => void) {
  rerun(computation(run));
}

/** A root of its own, disposed by hand: a row of a list, a card. */
export function root<T>(make: (dispose: () => void) => T, card: CardRoot | null = owner?.card ?? null): T {
  const made: Owner = { owner, card, disposed: false };
  const [lastListener, lastOwner] = [listener, owner];
  [listener, owner] = [null, made];
  try {
    return make(() => dispose(made));
  } finally {
    [listener, owner] = [lastListener, lastOwner];
  }
}

/** Runs `cleanup` when what runs now goes: the view, a part `Show` shows, a row of a `For`, a binding that runs again, or the node whose handler runs. */
export function onCleanup(cleanup: () => void) {
  if (owner?.disposed) cleanup();
  else if (owner) (owner.cleanups ??= []).push(cleanup);
}

/** Runs `run` as part of `scope`, following nothing it reads: what it makes goes when `scope` does. */
export function runIn<T>(scope: Owner | null, run: () => T): T {
  const [lastListener, lastOwner, lastSetup] = [listener, owner, settingUp];
  [listener, owner, settingUp] = [null, scope, 0];
  try {
    return run();
  } finally {
    [listener, owner, settingUp] = [lastListener, lastOwner, lastSetup];
  }
}

/** Reads once, without what runs now following it: what a component takes as it first draws, such as where an editor opens. */
export function untrack<T>(read: () => T): T {
  return runIn(owner, read);
}

/** The shortest wait of a timer: each run of one redraws its card. */
const SHORTEST_TIMER_MS = 100;

/**
 * Runs `run` once after `ms` (100 or more), unless what made it goes first, as `onCleanup`
 * tells: a delayed hide. Made in a view, a component, or a node's handler or drag. Gives what
 * stops it sooner.
 */
export function timeout(run: () => void, ms: number): () => void {
  return timer(run, ms, false);
}

/** Runs `run` every `ms` (100 or more) until what made it goes, as `timeout`: a button held to repeat. Gives what stops it sooner. */
export function interval(run: () => void, ms: number): () => void {
  return timer(run, ms, true);
}

function timer(run: () => void, ms: number, repeats: boolean): () => void {
  const scope = owner;
  if (!scope) throw new Error("a timer is made in a view, a component, or a node's handler or drag");
  if (!(ms >= SHORTEST_TIMER_MS)) {
    throw new Error(`a timer waits ${SHORTEST_TIMER_MS} ms or more, not ${ms}: for something that moves every frame, give a shape a live value from \`now\``);
  }
  if (scope.disposed) return () => {};
  const fire = () => {
    if (!repeats) stop();
    try {
      batch(() => runIn(scope, run));
    } catch (error) {
      failedIn(scope, error);
      host.changed();
    }
  };
  const handle = repeats ? setInterval(fire, ms) : setTimeout(fire, ms);
  const stop = () => {
    if (repeats) clearInterval(handle);
    else clearTimeout(handle);
    const at = scope.cleanups?.indexOf(stop) ?? -1;
    if (at >= 0) scope.cleanups?.splice(at, 1);
  };
  (scope.cleanups ??= []).push(stop);
  return stop;
}

/** Runs the body of a component: once, reading nothing that changes. */
export function setup<T>(body: () => T): T {
  const [lastListener] = [listener];
  listener = null;
  settingUp += 1;
  try {
    return body();
  } finally {
    settingUp -= 1;
    listener = lastListener;
  }
}

/** A value of the card's own that bindings follow: `const [open, setOpen] = signal(false)`. */
export function signal<T>(initial: T): [Accessor<T>, (next: T) => void] {
  let value = initial;
  const read = dep();
  return [
    () => {
      track(read);
      return value;
    },
    (next) => {
      if (!Object.is(value, next)) {
        value = next;
        notify(read);
      }
    },
  ];
}

/** A value worked out from others once per change, for many bindings to read. */
export function memo<T>(work: () => T): Accessor<T> {
  const [read, write] = signal<T>(undefined as T);
  effect(() => write(work()));
  return read;
}

/**
 * Whether a key is in a set that changes, such as the selected notes: a binding that asks for
 * one key runs again only when that key comes or goes, not when any does.
 */
export function selector<K>(keys: Accessor<ReadonlySet<K>>): (key: K) => boolean {
  const watched = new Map<K, Dep>();
  let current: ReadonlySet<K> = new Set();
  effect(() => {
    const next = keys();
    const last = current;
    current = next;
    for (const key of last) if (!next.has(key)) notify(watched.get(key));
    for (const key of next) if (!last.has(key)) notify(watched.get(key));
  });
  return (key) => {
    let read = watched.get(key);
    if (!read) {
      read = dep();
      watched.set(key, read);
    }
    track(read);
    return current.has(key);
  };
}

// Stores: the record, memory, outputs and the transport, read field by field.

export const RAW = Symbol("raw");
const ALL_KEYS = Symbol("keys");
const proxies = new WeakMap<object, object>();
const fieldSources = new WeakMap<object, Map<PropertyKey, Dep>>();

/** A proxy hears an index of a list as text: `"3"`, never `3`. */
function keyOf(key: PropertyKey): PropertyKey {
  return typeof key === "number" ? String(key) : key;
}

function fieldSource(target: object, key: PropertyKey): Dep {
  key = keyOf(key);
  let sources = fieldSources.get(target);
  if (!sources) {
    sources = new Map();
    fieldSources.set(target, sources);
  }
  let found = sources.get(key);
  if (!found) {
    found = dep();
    sources.set(key, found);
  }
  return found;
}

function changed(target: object, key: PropertyKey) {
  notify(fieldSources.get(target)?.get(keyOf(key)));
}

function isWrapped(value: unknown): value is object {
  if (value === null || typeof value !== "object") return false;
  const prototype = Object.getPrototypeOf(value);
  return Array.isArray(value) || prototype === Object.prototype || prototype === null;
}

function raw<T>(value: T): T {
  return (value !== null && typeof value === "object" && (value as Record<symbol, unknown>)[RAW]) as T || value;
}

/** A store over `target`: reads are tracked; writes go through only when `writable`. */
export function store<T extends object>(target: T, writable: boolean): T {
  // Already a store, such as a note of the record kept in memory.
  if ((target as Record<symbol, unknown>)[RAW]) return target;
  const known = proxies.get(target);
  if (known) return known as T;
  const proxy = new Proxy(target, {
    get(object, key, receiver) {
      if (key === RAW) return object;
      const value = Reflect.get(object, key, receiver);
      if (typeof key === "symbol" || (typeof value === "function" && !Object.hasOwn(object, key))) return value;
      // The name and the tool of a child never change: a rename is another record. So a
      // component may read them as it is made, as the key of a row.
      if ((key === "name" || key === "tool") && Object.hasOwn(object, "state")) return value;
      track(fieldSource(object, key));
      return isWrapped(value) ? store(value, writable) : value;
    },
    has(object, key) {
      if (typeof key !== "symbol") track(fieldSource(object, key));
      return Reflect.has(object, key);
    },
    ownKeys(object) {
      track(fieldSource(object, ALL_KEYS));
      return Reflect.ownKeys(object);
    },
    set(object, key, value) {
      if (!writable) {
        throw new TypeError(`${String(key)} is read only here: change the record with update(label, ({ state }) => ...)`);
      }
      const fresh = !Object.hasOwn(object, key);
      const next = raw(value);
      const last = (object as Record<PropertyKey, unknown>)[key];
      const lengthBefore = Array.isArray(object) ? object.length : 0;
      Reflect.set(object, key, next);
      if (!Object.is(last, next)) changed(object, key);
      if (Array.isArray(object) && object.length !== lengthBefore) changed(object, "length");
      if (fresh || (Array.isArray(object) && !Object.is(last, next))) changed(object, ALL_KEYS);
      return true;
    },
    deleteProperty(object, key) {
      if (!writable) throw new TypeError(`${String(key)} is read only here`);
      const had = Object.hasOwn(object, key);
      Reflect.deleteProperty(object, key);
      if (had) {
        changed(object, key);
        changed(object, ALL_KEYS);
      }
      return true;
    },
  });
  proxies.set(target, proxy);
  return proxy;
}

function sameShallow(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (!isWrapped(a) || !isWrapped(b) || Array.isArray(a) !== Array.isArray(b)) return false;
  const [left, right] = [a as Record<string, unknown>, b as Record<string, unknown>];
  const keys = Object.keys(left);
  return keys.length === Object.keys(right).length && keys.every((key) => sameShallow(left[key], right[key]));
}

/** The value each part of a store was last made like: one given again is the same, and is not walked. */
const madeFrom = new WeakMap<object, object>();

/**
 * Makes `target` hold what `next` holds, keeping each object that stays, so only bindings of
 * what changed run. A list of the same length keeps its items by place, so a moved note is the
 * same note, unless a child there has another name; one that grew or shrank keeps the items
 * that did not change by what they hold, so a note deleted before others leaves them be.
 */
export function reconcile(target: Record<PropertyKey, unknown>, next: Record<PropertyKey, unknown>) {
  // Frozen values are kept as they are, so a record that did not change, and all it holds, is
  // the same object as last time: the clips of the tracks an edit left alone.
  if (madeFrom.get(target) === next) return;
  if (Object.isFrozen(next)) madeFrom.set(target, next);
  if (Array.isArray(target) && Array.isArray(next)) {
    reconcileList(target, next);
    return;
  }
  let keysChanged = false;
  for (const key of Object.keys(target)) {
    if (!Object.hasOwn(next, key)) {
      delete target[key];
      changed(target, key);
      keysChanged = true;
    }
  }
  for (const [key, value] of Object.entries(next)) {
    keysChanged ||= !Object.hasOwn(target, key);
    reconcileField(target, key, value);
  }
  if (keysChanged) changed(target, ALL_KEYS);
}

function reconcileField(target: Record<PropertyKey, unknown>, key: PropertyKey, value: unknown) {
  const last = target[key];
  if (isWrapped(last) && isWrapped(value) && Array.isArray(last) === Array.isArray(value)) {
    reconcile(last as Record<PropertyKey, unknown>, value as Record<PropertyKey, unknown>);
  } else if (!Object.is(last, value)) {
    target[key] = adopted(value);
    changed(target, key);
    if (Array.isArray(target)) changed(target, ALL_KEYS);
  }
}

/**
 * A value of `next` as a store keeps it: its own copy, which a later reconcile changes in place.
 * Each copy of a frozen value remembers it, so the next reconcile skips what did not change
 * instead of walking all of it once.
 */
export function adopted<T>(value: T): T {
  if (!isWrapped(value)) return value;
  const copy: Record<PropertyKey, unknown> = Array.isArray(value) ? ([] as never) : {};
  for (const [key, each] of Object.entries(value)) copy[key] = adopted(each);
  if (Object.isFrozen(value)) madeFrom.set(copy, value);
  return copy as T;
}

/** What an item of a list is called: a child by its name and tool. Other items have none, and a list keeps them by place. */
function idOf(value: unknown): string | undefined {
  if (!isWrapped(value) || Array.isArray(value)) return undefined;
  const { name, tool } = value as { name?: unknown; tool?: unknown };
  return typeof name === "string" ? `${name}:${String(tool)}` : undefined;
}

function reconcileList(target: unknown[], next: unknown[]) {
  // A child swapped for another of the same place is another item: its rows are made again.
  if (target.length === next.length && next.every((value, index) => idOf(value) === idOf(target[index]))) {
    next.forEach((value, index) => reconcileField(target as never, index, value));
    return;
  }
  const lengthBefore = target.length;
  let start = 0;
  while (start < target.length && start < next.length && sameShallow(target[start], next[start])) start++;
  let endTarget = target.length;
  let endNext = next.length;
  while (endTarget > start && endNext > start && sameShallow(target[endTarget - 1], next[endNext - 1])) {
    endTarget--;
    endNext--;
  }
  const byContent = new Map<string, unknown[]>();
  for (let index = start; index < endTarget; index++) {
    const key = JSON.stringify(target[index]);
    byContent.set(key, [...(byContent.get(key) ?? []), target[index]]);
  }
  const middle = next.slice(start, endNext).map((value) => {
    const same = byContent.get(JSON.stringify(value))?.shift();
    return same === undefined ? { value } : { kept: same };
  });
  const unused = [...byContent.values()].flat();
  const result = middle.map((each) => {
    if ("kept" in each) return each.kept;
    const reused = isWrapped(each.value)
      ? unused.find((old) => isWrapped(old) && Array.isArray(old) === Array.isArray(each.value) && idOf(old) === idOf(each.value))
      : undefined;
    if (reused === undefined) return adopted(each.value);
    unused.splice(unused.indexOf(reused), 1);
    reconcile(reused as Record<PropertyKey, unknown>, each.value as Record<PropertyKey, unknown>);
    return reused;
  });
  const before = target.slice();
  target.splice(start, endTarget - start, ...result);
  target.forEach((value, index) => {
    if (!Object.is(before[index], value)) changed(target, index);
  });
  for (let index = target.length; index < before.length; index++) changed(target, index);
  if (target.length !== lengthBefore) changed(target, "length");
  changed(target, ALL_KEYS);
}

/** The fields of a store that bindings read now, or `null` when one reads them all. */
export function readFields(target: object): string[] | null {
  const sources = fieldSources.get(target);
  if (!sources) return [];
  if ((sources.get(ALL_KEYS)?.observers.size ?? 0) > 0) return null;
  return [...sources].filter(([key, read]) => typeof key === "string" && read.observers.size > 0).map(([key]) => key as string);
}

export function isRead(target: object): boolean {
  const fields = readFields(target);
  return fields === null || fields.length > 0;
}
