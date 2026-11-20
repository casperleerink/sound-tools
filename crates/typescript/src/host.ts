// Runs the project's own tools, in Bun, with the `extensions` folder of the project as its
// working folder. It loads the files once: a change of the code starts a new Bun. One JSON
// message per line: the runtime tells on stdin, this says on stdout. What a tool logs goes to
// stderr, so it cannot break a message.

import { readFileSync, realpathSync, writeSync } from "node:fs";
import { join } from "node:path";
import type { Clock as SdkClock, Take, Context, PlainKey, Field, Fields, Graph, HeardEvent, Inputs, NoteEvent, OwnedChild, Part, Performer, Section, TempoMap, ToolSpec, ValuesOf } from "./sdk";

const folder = process.cwd();
/** The files of tools to load, by name: the runtime leaves out those whose code hung or stopped Bun. */
const [bunNumber, ...files] = process.argv.slice(2);
/** Each file as a call stack names it, with links resolved. */
const stackPaths = files.map((file) => [file, `${realpathSync(join(folder, file))}:`] as const);
// So the stack `tool` keeps reaches its file through any number of helpers.
Error.stackTraceLimit = Infinity;
const sdk: typeof import("./sdk") = await import(join(folder, "sdk.ts"));
/** What the host uses of `sdk/view.ts`: a tool's `view` is one of it. */
type Views = {
  host: {
    run(instance: string, draw: unknown, self: unknown, ctx: unknown): unknown;
    perform(instance: string, key: string, action: string, value?: unknown): void;
    forget(instance: string): void;
    refuseHeld(held: readonly string[], before: object, after: object): void;
  };
};
/** What the window knows of an instance that no record holds, see `Facts` in `sdk/view.ts`. */
type Facts = { title: string; fields: { name: string; default?: unknown }[] };
const views: Views = await import(join(folder, "sdk/view.ts"));

// Code of an older Sound Tools imports parts of its views that the SDK no longer has. Its files
// load with those names empty, and its tools without their views: their sound plays, and a card
// draws them, until an agent writes the views again.
/** Every name the SDK exports, values and types. */
const sdkNames = new Set(
  [...readFileSync(join(folder, "sdk.ts"), "utf8").matchAll(/export (?:type )?\{([^}]*)\}/g)]
    .flatMap(([, list]) => list!.split(","))
    .map((name) => name.trim().split(/\s+as\s+/).pop()!)
    .concat([...readFileSync(join(folder, "sdk/records.ts"), "utf8").matchAll(/export (?:declare )?(?:type|interface|const|function|enum) (\w+)/g)].map(([, name]) => name!)),
);
/** The files, by their real path, that import a name the SDK no longer has. */
const oldViews = new Set<string>();
const realFolder = realpathSync(folder);
Bun.plugin({
  setup(build) {
    build.onLoad({ filter: /\.tsx?$/ }, ({ path }) => {
      const text = readFileSync(path, "utf8");
      const loader = path.endsWith(".tsx") ? "tsx" : "ts";
      if (!path.startsWith(`${realFolder}/`) || path.startsWith(join(realFolder, "sdk"))) return { contents: text, loader };
      // A name with no value: a type, which Bun leaves out unless the code uses it as a value, or gone.
      const empty: string[] = [];
      const kept = text.replace(/import\s*\{([^}]*)\}\s*from\s*(["'])((?:\.\.?\/)+sdk)\2/g, (whole: string, list: string, quote: string, from: string) => {
        const parts = list.replace(/\/\/[^\n]*|\/\*[\s\S]*?\*\//g, "").split(",").map((part) => part.trim());
        const known = parts.filter((part) => {
          const [name, local = name] = part.split(/\s+as\s+/);
          if (part === "" || part.startsWith("type ") || name! in sdk) return part !== "";
          empty.push(local!);
          if (!sdkNames.has(name!)) oldViews.add(path);
          return false;
        });
        // On as many lines as it was, so an error names the line of the file.
        return `import { ${known.join(", ")} } from ${quote}${from}${quote};${"\n".repeat(whole.split("\n").length - 1)}`;
      });
      if (empty.length === 0) return { contents: text, loader };
      return { contents: `const ${empty.map((name) => `${name}: any = undefined`).join(", ")}; ${kept}`, loader };
    });
  },
});
const isOld = (file: string) => oldViews.has(realpathSync(join(folder, file)));

for (const name of ["log", "info", "debug", "warn"] as const) {
  console[name] = (...values: unknown[]) => console.error(...values);
}
// Code of a tool that fails after an `await`, where nothing waits for it, must not stop Bun.
process.on("unhandledRejection", (error) => console.error(error));

type Spec = ToolSpec<Fields, Inputs, unknown>;
type State = ValuesOf<Fields>;
type Choices = Record<string, string | number>;
/** An instance whose code runs. */
type Played = { instance: string };
/** A record as the runtime tells it: its `state` as the file holds it. */
type Told = { id: string; tool: string; state: unknown };
/** Where the piece plays and how, as the window tells it. */
type Transport = { playing: boolean; tick: number; bar: number; beat: number; bpm: number; repeat: [number, number] | null; click: boolean; keys: boolean; automation: boolean };
/** Where the sound and the piece are now, which all code of a tool gets. */
type Clock = { time: number; transport: Transport };
/** The tempo map and the sections, as `project.json` holds them. */
type Song = { tempo_map: TempoMap; sections: Section[] };

type Request =
  /** The records this Bun did not hold as they are, those that are gone, and how many of its edits the runtime heard. */
  | { type: "records"; records: Told[]; gone: string[]; edits: number }
  | ({ type: "clock" } & Clock)
  | ({ type: "song" } & Song)
  | ({ type: "frame"; dt: number; instances: Played[] } & Clock)
  | ({ type: "key"; instance: Played; key: PlainKey; down: boolean; shift: boolean; alt: boolean; cmd: boolean; ctrl: boolean } & Clock)
  /** The key of these actions of its tool went down: they run in this order until one does it. */
  | ({ type: "action"; instance: Played; actions: string[] } & Clock)
  | ({ type: "notes"; instance: Played; events: HeardEvent[] } & Clock)
  | ({ type: "take"; instance: Played } & Take & Clock)
  | { type: "behaviour"; question: number; tool: string; instance: string }
  /** The answer to a question of `ask`. */
  | { type: "answer"; question: number; answer: unknown }
  /**
   * The instances whose views the window shows: each sends its tree now, and again when its
   * records change. `cards` names those a card draws; `facts` are those of each and of what
   * it holds at any depth, `catalog` what `addOffer` puts, `keys` each action's key by `tool.action`.
   */
  | { type: "show"; instances: string[]; cards: string[]; held: Record<string, string[]>; facts?: Record<string, Facts>; catalog?: unknown[]; keys?: Record<string, string | null> }
  /** An action on an element of a shown view, by its key: `press` and `drag` with a pointer, `drop` with files, `commit` with text or `null`, or one it names with a number. */
  | { type: "act"; instance: string; key: string; action: string; value?: unknown };

/** Says something to the runtime, at once: before code that may hang runs, and in order. */
function say(message: object) {
  writeSync(1, JSON.stringify(message) + "\n");
}

/** Tells the runtime whose code runs now, so it knows what to blame when it hangs. */
function runs(tool: string, what: string) {
  say({ type: "runs", tool, what });
}

/** What the control loop and the cards of each instance keep. */
const memories = new Map<string, unknown>();
/** The tools whose tick or a click on whose card failed. */
const failed = new Set<string>();

/** Tells the runtime the first failure of a tool: it lists it as a problem. */
function fail(tool: string, message: string) {
  if (!failed.has(tool)) {
    failed.add(tool);
    console.error(`${tool}: ${message}`);
    say({ type: "failed", tool, message });
  }
}

const NAME = /^[a-z][a-z0-9_]*$/;
/** The most combinations of choices a tool has: the sound of each is made when it loads. */
const MAX_SOUNDS = 256;
/** Every key of a tool, so a misspelled or an old one is a problem instead of left out. */
const KEYS = {
  name: true, title: true, when: true, doc: true, voices: true, state: true, inputs: true, outputs: true, sound: true,
  behaviour: true, children: true, memory: true, tick: true, view: true, actions: true, onKey: true,
  keys: true, globalKeys: true, onNotes: true, onTake: true,
} satisfies Record<keyof Spec, true>;
/** The keys the app keeps, on macOS and elsewhere: no tool takes them. */
const APP_KEYS = ["z", "n", "o", "q", "l", "b", "k"].flatMap((key) => [`cmd-${key}`, `ctrl-${key}`]).concat(["cmd-shift-z", "cmd-alt-z", "cmd-,", "tab", "shift-tab"]);
/** The most voices a sound plays at once. */
const MAX_VOICES = 8;

/**
 * What is wrong with the definition of a tool that its types do not say, so the agent that
 * wrote it can fix it. The runtime checks the name, the kinds and the ranges.
 */
function problemsOf(spec: Spec): string[] {
  const problems: string[] = [];
  try {
    for (const key of Object.keys(spec)) {
      if (!(key in KEYS)) {
        problems.push(`${key}: a tool has no ${key}; its keys are ${Object.keys(KEYS).join(", ")}`);
      }
    }
    for (const key of ["title", "when", "doc"] as const) {
      if (!spec[key]?.trim()) {
        problems.push(`${key}: write it, it is what the composer and agents read`);
      }
    }
    const names = [...Object.keys(spec.state), ...Object.keys(spec.inputs ?? {})];
    for (const name of names) {
      if (!NAME.test(name)) {
        problems.push(`${name}: a field or input name is lowercase letters, digits and _, starting with a letter`);
      }
      if (names.indexOf(name) !== names.lastIndexOf(name)) {
        problems.push(`${name}: a field and an input cannot share a name`);
      }
    }
    for (const name of [...Object.keys(spec.inputs ?? {}), ...Object.keys(spec.outputs ?? {})]) {
      if (name === "audio" || name === "notes") {
        problems.push(`${name}: an input or an output is not named ${name}, which every sound has`);
      }
    }
    for (const name of Object.keys(spec.outputs ?? {})) {
      if (!NAME.test(name)) {
        problems.push(`${name}: an output name is lowercase letters, digits and _, starting with a letter`);
      }
    }
    const voices = spec.voices ?? 1;
    if (!Number.isInteger(voices) || voices < 1 || voices > MAX_VOICES) {
      problems.push(`voices: ${voices} is not a whole number from 1 to ${MAX_VOICES}`);
    }
    const sounds = choiceFields(spec).reduce((count, [, field]) => count * field.options.length, 1);
    if (sounds > MAX_SOUNDS) {
      problems.push(`its choices make ${sounds} sounds, at most ${MAX_SOUNDS}: make some of them knobs or toggles`);
    }
    if (spec.behaviour && voices !== 1) {
      problems.push('voices: a parent\'s sound is one voice that hears what it connects to its own { port: "audio" } as `input`, so leave `voices` out');
    }
    for (const [list, names] of [["keys", spec.keys ?? []], ["globalKeys", spec.globalKeys ?? []]] as const) {
      for (const name of names) {
        if (!sdk.host.isKeyName(name)) {
          problems.push(`${list}: ${name} is no key; name one as "a", "space", "left" or "cmd-c", with cmd, ctrl and alt first`);
        }
      }
    }
    for (const key of spec.globalKeys ?? []) {
      if (APP_KEYS.includes(key)) {
        problems.push(`globalKeys: ${key} is the app's: undo, redo, new, open, quit, settings, the panel, the computer keys and tab are`);
      }
    }
    if (spec.actions && spec.name === "app") {
      problems.push("actions: the actions of `app` are the app's own, so give the tool another name");
    }
    for (const [name, action] of Object.entries(spec.actions ?? {})) {
      if (!NAME.test(name)) {
        problems.push(`actions: ${name} is no name; an action name is lowercase letters, digits and _, starting with a letter`);
      }
      if (!action.title?.trim()) {
        problems.push(`actions.${name}: write its title, it is what the composer reads in the list of shortcuts`);
      }
      for (const key of action.keys ?? []) {
        if (APP_KEYS.includes(key)) {
          problems.push(`actions.${name}: ${key} is the app's and never reaches a tool: undo, redo, new, open, quit, settings, the panel, the computer keys and tab are`);
        } else if (!sdk.host.isActionKey(key)) {
          problems.push(`actions.${name}: ${key} is no key; name one as "a", "space", "shift-up" or "cmd-shift-c", with cmd, alt and shift first, in that order, and a symbol with shift by what it types, "?"; cmd is ctrl on Windows and Linux, so there is no ctrl-`);
        }
      }
    }
    if (spec.children && !spec.behaviour) {
      problems.push("children: only a parent has children: give it a behaviour that connects them");
    }
  } catch (error) {
    // A definition its types refuse, such as one without `state`.
    problems.push(String(error));
  }
  return problems;
}

/** The file of `files` whose code called `tool` for `name`, else `loading`, the file that ran it. */
function definedIn(name: string, loading: string): string {
  for (const frame of (sdk.host.stacks.get(name) ?? "").split("\n")) {
    const found = stackPaths.find(([, path]) => frame.includes(path));
    if (found !== undefined) {
      return found[0];
    }
  }
  return loading;
}

/** Loads the files, each after it says which, so a file that hangs is known. */
async function load() {
  const tools: object[] = [];
  const errors: Array<{ file: string; message: string }> = [];
  for (const file of files) {
    say({ type: "loading", file });
    const before = new Set(sdk.host.tools.keys());
    try {
      const module = await import(join(folder, file));
      const card = file === GENERIC_CARD || file.startsWith(`${CARDS}/`);
      if (card && typeof module.card !== "function") throw new Error("it exports no `card`, a `Card`");
      if (file === GENERIC_CARD) genericCard = module.card;
      else if (card) toolCards.set(file.slice(CARDS.length + 1, -".tsx".length), module.card);
    } catch (error) {
      // The tools it defined before it failed are its tools too.
      errors.push({ file, message: String(error) });
    }
    for (const [name, spec] of sdk.host.tools) {
      if (before.has(name)) {
        continue;
      }
      const defined = definedIn(name, file);
      if (isOld(defined)) delete spec.view;
      const problems = problemsOf(spec);
      errors.push(...problems.map((problem) => ({ file: defined, message: `tool ${name}: ${problem}` })));
      if (problems.length === 0) {
        tools.push(described(spec, defined));
      }
    }
  }
  say({ type: "loaded", tools, errors });
}

/**
 * A tool as the runtime reads it: what it is, the sound of every combination of its choices,
 * and the nodes of its view at its defaults, so a view that fails is a problem at
 * once. A function of the wrong type is called anyway, so it fails as a problem of its tool.
 */
function described(spec: Spec, file: string) {
  const inputs = Object.entries(spec.inputs ?? {});
  const sounds = combinations(spec).map((choices) => [choices, made(() => sound(spec, choices))] as const);
  beats.set(spec.name, sounds.some(([, graph]) => "Ok" in graph && readsBeat(graph.Ok)));
  // Before its actions are read: a `Transport` in its view gives it `play`.
  const view = made(() => checkView(spec));
  return {
    info: {
      name: spec.name,
      file,
      title: spec.title,
      when: spec.when,
      doc: spec.doc,
      voices: spec.voices ?? 1,
      input: sounds.some(([, graph]) => "Ok" in graph && readsInput(graph.Ok)),
      sound: spec.sound !== undefined,
      silent: sounds.length > 0 && sounds.every(([, graph]) => "Ok" in graph && isSilent(graph.Ok)),
      fields: spec.state,
      inputs: Object.fromEntries(inputs.filter(([, input]) => input.kind !== "audio")),
      sound_inputs: inputs.filter(([, input]) => input.kind === "audio").map(([name]) => name),
      tick: spec.tick !== undefined,
      view: spec.view !== undefined,
      old_views: isOld(file),
      keys: spec.onKey !== undefined,
      takes: spec.keys ?? null,
      global_keys: spec.globalKeys ?? [],
      actions: Object.fromEntries(Object.entries(spec.actions ?? {}).map(([name, { title, keys = [], everywhere = false }]) => [name, { title, keys, everywhere }])),
      notes: spec.onNotes !== undefined,
      on_take: spec.onTake !== undefined,
      behaviour: spec.behaviour !== undefined,
      children: spec.children,
    },
    sounds,
    view,
  };
}

/** Whether a sound hears what comes in to its `audio`, so its tool has an audio input. */
function readsInput(graph: Graph): boolean {
  return graph.nodes.some((node) => (node.op === "input" || node.op === "inputLeft" || node.op === "inputRight") && node.name === undefined);
}

/** Whether a sound plays in the bars of the piece: it reads its beat or its tempo. */
function readsBeat(graph: Graph): boolean {
  return graph.nodes.some((node) => node.op === "beat" || node.op === "bpm");
}

/** Whether the sound of each tool reads the beat, once worked out. */
const beats = new Map<string, boolean>();

// A `Transport` asks of the tool that draws it and of its children, some of which a file loaded later describes.
sdk.host.readsBeat = (tool) => {
  const spec = sdk.host.tools.get(tool);
  if (spec && !beats.has(tool)) {
    beats.set(tool, combinations(spec).some((choices) => {
      const graph = made(() => sound(spec, choices));
      return "Ok" in graph && readsBeat(graph.Ok);
    }));
  }
  return beats.get(tool) ?? false;
};

/** The parts of `note`, as a sound reads them. */
const NOTE_OPS = ["frequency", "pitch", "gate", "velocity", "onset"];

/** Whether notes play each tool of the project, once worked out. */
const hearing = new Map<string, boolean>();

sdk.host.hearsNotes = (tool) => {
  const spec = sdk.host.tools.get(tool);
  // A built-in tool in a slot for notes plays them.
  if (!spec) return true;
  if (!hearing.has(tool)) {
    // Its code hears them, its behaviour may pass them on to a child, or its sound reads them.
    hearing.set(tool, spec.onNotes !== undefined || spec.behaviour !== undefined || combinations(spec).some((choices) => {
      const graph = made(() => sound(spec, choices));
      return "Ok" in graph && graph.Ok.nodes.some((node) => NOTE_OPS.includes(node.op));
    }));
  }
  return hearing.get(tool) ?? true;
};

/** Whether a sound is silence and nothing else: it moves numbers, as an LFO does. */
function isSilent(graph: Graph): boolean {
  const output = "mono" in graph.output ? graph.nodes[graph.output.mono] : undefined;
  return graph.outputs.length === 0 && output?.op === "constant" && output.value === 0;
}

/** What `make` gives, or why it failed, as the runtime reads a `Result`. */
function made<T>(make: () => T): { Ok: T } | { Err: string } {
  try {
    return { Ok: make() };
  } catch (error) {
    return { Err: String(error) };
  }
}

type ChoiceField = Extract<Fields[string], { kind: "choice" }>;

function choiceFields(spec: Spec): Array<[string, ChoiceField]> {
  return Object.entries(spec.state).filter((entry): entry is [string, ChoiceField] => entry[1].kind === "choice");
}

/** Every combination of the choices of a tool. */
function combinations(spec: Spec): Choices[] {
  return choiceFields(spec).reduce<Choices[]>(
    (all, [name, field]) => all.flatMap((choices) => field.options.map((option) => ({ ...choices, [name]: option }))),
    [{}],
  );
}

function specOf(tool: string): Spec {
  const spec = sdk.host.tools.get(tool);
  if (!spec) {
    throw new Error(`no tool ${tool} is loaded`);
  }
  return spec;
}

/** The sound graph of a tool for these choices. */
function sound(spec: Spec, choices: Choices): Graph {
  const fields: Record<string, unknown> = {};
  for (const [name, field] of Object.entries(spec.state)) {
    if (field.kind === "number" || field.kind === "boolean") {
      fields[name] = new sdk.Param(name);
    } else if (field.kind === "choice") {
      fields[name] = choices[name] ?? field.default;
    } else if (field.kind === "file") {
      fields[name] = new sdk.Sample(name);
    } else if (field.kind === "list" && field.item.kind === "number") {
      fields[name] = new sdk.Table({ list: name });
    }
  }
  for (const [name, input] of Object.entries(spec.inputs ?? {})) {
    fields[name] = input.kind === "audio" ? new sdk.SoundInput(name) : new sdk.Param(name);
  }
  const build = spec.sound ?? (() => sdk.input);
  return sdk.graphToJson(() => build(fields as never), Object.keys(spec.outputs ?? {}));
}

/**
 * What a behaviour read of its record and its children: the fields of its own state, and the
 * fields of any child's state, each `null` when it read them all, as `Object.keys` does. The
 * names and tools of its children always count. The runtime asks again only when one of these
 * changed, so a fader dragged on a child does not ask a behaviour that reads only names.
 */
type BehaviourReads = { state: string[] | null; children: string[] | null };

/** `source` as a behaviour reads it, which notes each field it reads in `into`. */
function noting<T extends object>(source: T, into: { fields: Set<string> | null }): T {
  const note = (key: PropertyKey) => {
    if (typeof key === "string") into.fields?.add(key);
  };
  const all = () => {
    into.fields = null;
  };
  return new Proxy({} as T, {
    get: (_, key) => (note(key), Reflect.get(source, key)),
    has: (_, key) => (note(key), Reflect.has(source, key)),
    ownKeys: () => (all(), Reflect.ownKeys(source)),
    getOwnPropertyDescriptor: (_, key) => {
      note(key);
      const descriptor = Reflect.getOwnPropertyDescriptor(source, key);
      return descriptor && { ...descriptor, configurable: true };
    },
  });
}

/** `children` as a behaviour reads them, at any depth: the fields of the state it reads are what the runtime compares of each. */
function notingChildren(children: OwnedChild[], into: { fields: Set<string> | null }): OwnedChild[] {
  return children.map((child) => {
    let state: unknown;
    let inside: OwnedChild[] | undefined;
    return Object.freeze({
      name: child.name,
      tool: child.tool,
      get state() {
        return (state ??= isObject(child.state) ? noting(child.state, into) : child.state);
      },
      get children() {
        return (inside ??= notingChildren(child.children, into));
      },
    }) as OwnedChild;
  });
}

/**
 * Answers what the behaviour of a parent declares for its record and its children as the
 * runtime told them, which its question is for: not with the edits this Bun sent since.
 */
function declare({ question, tool, instance }: Extract<Request, { type: "behaviour" }>) {
  const own = { fields: new Set<string>() as Set<string> | null };
  const inside = { fields: new Set<string>() as Set<string> | null };
  const declaration = made(() => {
    const spec = specOf(tool);
    if (!spec.behaviour) {
      throw new Error(`${tool} has no behaviour`);
    }
    const record = told.get(instance);
    if (!record) {
      throw new Error(`the runtime told no record of ${instance}`);
    }
    runs(tool, "behaviour");
    const answer = spec.behaviour({
      state: noting(filledState(spec, record), own),
      children: notingChildren(childrenOf(instance, false), inside),
    });
    // JSON leaves out a `child` that is undefined, which would make the end a port of the parent.
    const connections: unknown[] = Array.isArray(answer?.connections) ? answer.connections : [];
    connections.forEach((connection, index) => {
      for (const side of ["from", "to"] as const) {
        const end = (connection as Record<string, unknown> | null)?.[side];
        if (end !== null && typeof end === "object" && "child" in end && typeof end.child !== "string") {
          throw new Error(
            `connections[${index}].${side}.child is ${String(end.child)}: name a child, or leave child out for a port of the parent itself`,
          );
        }
      }
    });
    // Here, so a value JSON cannot hold, such as a cycle, is the parent's problem and does not throw in `say`.
    return JSON.parse(JSON.stringify(answer));
  });
  const list = (read: { fields: Set<string> | null }) => (read.fields ? [...read.fields].sort() : null);
  const reads: BehaviourReads = { state: list(own), children: list(inside) };
  say({ type: "declared", question, declaration, reads });
}

/** The memory of an instance, made when it is first needed. */
function memoryOf(instance: string, spec: Spec): unknown {
  // A record that becomes another tool's starts its memory again.
  const key = `${instance} ${spec.name}`;
  if (!memories.has(key)) {
    memories.set(key, freshMemory(spec));
  }
  return memories.get(key);
}

function freshMemory(spec: Spec): unknown {
  return spec.memory ? spec.memory() : {};
}

/** The `time` of the last frame. */
let now = 0;
/** How far ahead `at` may be: more is a mistake, such as milliseconds, that would hold the list of what is to come. */
const AHEAD = 10;

/** Throws when `at` is too far ahead to be meant. */
function checkAt(at: number | undefined) {
  if (at !== undefined && at > now + AHEAD) {
    throw new RangeError(`at is ${at}, ${(at - now).toFixed(1)} s after time: give a time of the loop in seconds, at most ${AHEAD} s ahead`);
  }
}

/** An input as `send` names it, as the runtime reads it: the child it is of, its name, and what it carries. */
function inputOf(input: unknown): { child?: string; name: string; kind?: "number" | "bang" | "notes" } {
  if (typeof input === "string") return { name: input, kind: input === "notes" ? "notes" : undefined };
  if (input !== null && typeof input === "object" && "child" in input && typeof input.child === "string") {
    for (const kind of ["number", "bang", "notes"] as const) {
      const name = (input as Record<string, unknown>)[kind];
      if (typeof name === "string") return { child: input.child, name, kind };
    }
  }
  throw new TypeError(`send plays an input: its name, "notes", or { child, number }, { child, bang } or { child, notes }, not ${JSON.stringify(input)}`);
}

/** What a view and the control loop use to play an instance. */
function performer(instance: string): Performer<Inputs> {
  const notes = (event: NoteEvent, at: number | undefined, child: string | undefined) => {
    checkAt(at);
    say({ type: "send", instance, child, event: wired(event), at });
  };
  return {
    send: ((input: unknown, ...rest: unknown[]) => {
      const { child, name, kind } = inputOf(input);
      const [first, second] = rest as [unknown, { at?: number } | undefined];
      if (kind === "notes") {
        notes(first as NoteEvent, second?.at, child);
        return;
      }
      // A number input takes a number; a bang takes only when it happens.
      const value = typeof first === "number" ? finite(first, name) : undefined;
      if (kind === "number" && value === undefined) {
        throw new TypeError(`send({ child: "${child}", number: "${name}" }, value) takes a number`);
      }
      const { at } = ((value === undefined ? first : second) ?? {}) as { at?: number };
      checkAt(at);
      say({ type: "control", instance, child, name, value, at });
    }) as Performer<Inputs>["send"],
    play(pitch, { seconds = 0.25, hold = false, velocity = 0.8, at, child } = {}) {
      checkAt(at);
      if (hold) {
        notes({ type: "noteOn", pitch, velocity }, at, child);
      } else {
        say({ type: "note", instance, child, pitch: keyOf(finite(pitch, "pitch")), velocity: finite(velocity, "velocity"), seconds: finite(seconds, "seconds"), at });
      }
    },
  };
}

/** The MIDI key of a pitch. */
function keyOf(pitch: number): number {
  return Math.max(0, Math.min(127, Math.round(pitch)));
}

/** `value` when it is a number JSON can hold, which `NaN` is not. */
function finite(value: number, name: string): number {
  if (!Number.isFinite(value)) {
    throw new RangeError(`${name} is ${value}: give a number`);
  }
  return value;
}

/** A note event as the runtime reads it: only its own keys, and a whole key and controller. */
function wired(event: NoteEvent): NoteEvent {
  switch (event.type) {
    case "noteOn":
      return { type: event.type, pitch: keyOf(finite(event.pitch, "pitch")), velocity: finite(event.velocity, "velocity") };
    case "noteOff":
      return { type: event.type, pitch: keyOf(finite(event.pitch, "pitch")) };
    case "cc": {
      const { controller } = event;
      if (!Number.isInteger(controller) || controller < 0 || controller > 119) {
        throw new RangeError(`controller is ${controller}: a MIDI controller is a whole number from 0 to 119`);
      }
      return { type: event.type, controller, value: finite(event.value, "value") };
    }
    case "bend":
    case "pressure":
      return { type: event.type, value: finite(event.value, "value") };
    case "allOff":
      return { type: event.type };
    default:
      throw new TypeError(`${JSON.stringify(event)} is no note event: its type is noteOn, noteOff, cc, bend, pressure or allOff`);
  }
}

// ---------------------------------------------------------------------------------------------
// The records: what the runtime told, and on top the edits this Bun sent that it has not heard
// yet. What code gets is frozen, so it changes a record only through `update`.

type Entry = { tool: string; state: unknown };
/** One record an edit changes, by its id. The runtime gets the child by name. */
type Change =
  | { type: "fields"; id: string; set: Record<string, unknown>; removed: string[] }
  | { type: "record"; id: string; tool: string; state: unknown }
  | { type: "delete"; id: string };

const told = new Map<string, Entry>();
/** The names of the children of each instance told, and their list as told, made when first read. */
const kids = new Map<string, { names: Set<string>; told?: OwnedChild[] }>();
/** The edits sent that the runtime has not said it heard, oldest first, by their number. */
let unheard: Array<{ number: number; changes: Change[] }> = [];
let edits = 0;

function frozen<T>(value: T): T {
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    for (const each of Object.values(value)) {
      frozen(each);
    }
    Object.freeze(value);
  }
  return value;
}

function parentOf(id: string): [string, string] | undefined {
  const at = id.lastIndexOf("/");
  return at < 0 ? undefined : [id.slice(0, at), id.slice(at + 1)];
}

/** `ids` and every instance they are inside: those whose children change with them. */
function withOwners(ids: Iterable<string>): Set<string> {
  const all = new Set<string>();
  for (const id of ids) {
    all.add(id);
    for (let owner = parentOf(id); owner !== undefined; owner = parentOf(owner[0])) {
      all.add(owner[0]);
    }
  }
  return all;
}

/** Forgets the memory of `id`, which is gone or another tool's now. */
function forget(id: string) {
  for (const key of memories.keys()) {
    if (key.startsWith(`${id} `)) memories.delete(key);
  }
  for (const key of lastMade.keys()) {
    if (key.startsWith(`${id} `)) lastMade.delete(key);
  }
}

function hear({ records, gone, edits: heard }: Extract<Request, { type: "records" }>) {
  recordsChanged += 1;
  for (const { id, tool, state } of records) {
    if (told.get(id)?.tool !== tool) forget(id);
    told.set(id, { tool, state });
    const [parent, name] = parentOf(id) ?? [];
    if (parent !== undefined && name !== undefined) {
      kids.set(parent, { names: (kids.get(parent)?.names ?? new Set()).add(name) });
    }
  }
  for (const id of gone) {
    told.delete(id);
    kids.delete(id);
    forget(id);
    const [parent, name] = parentOf(id) ?? [];
    const known = parent === undefined ? undefined : kids.get(parent);
    if (parent !== undefined && name !== undefined && known) {
      known.names.delete(name);
      kids.set(parent, { names: known.names });
    }
  }
  // An edit the runtime refused changed nothing: what it showed goes back to what was told.
  const refused = unheard.filter(({ number }) => number <= heard).flatMap(({ changes }) => changes.map((change) => change.id));
  unheard = unheard.filter(({ number }) => number > heard);
  const touched = withOwners([...records.map((record) => record.id), ...gone, ...refused]);
  // The lists of children made before hold what changed, also those of an owner far up.
  for (const id of touched) {
    const known = kids.get(id);
    if (known) known.told = undefined;
  }
  for (const id of touched) {
    if (shown.has(id)) drawNext(id);
  }
}

// ---------------------------------------------------------------------------------------------
// Views (`sdk/view.ts`): a plain function of the records, run again on every
// change of them. The window gets the whole tree; what a `memo` kept it has already.

const shown = new Set<string>();

/** Runs the view of `instance` and sends its tree, or that its tool has none. */
function drawNext(instance: string) {
  Object.values(followsClock).forEach((views) => views.delete(instance));
  followsSong.delete(instance);
  drawing = instance;
  try {
    draw(instance);
  } finally {
    drawing = undefined;
  }
}

function draw(instance: string) {
  const record = recordOf(instance);
  const spec = sdk.host.tools.get(record?.tool ?? "");
  // The runtime says which instances a card draws: it and Bun read one list.
  if (record && carded.has(instance)) {
    // Without a generic card that loaded, the runtime says why in its problems.
    if (genericCard) drawCard(instance, record);
    else say({ type: "tree", instance, none: true });
    return;
  }
  if (!record || !spec?.view) {
    say({ type: "tree", instance, none: true });
    return;
  }
  const self = { name: nameOf(instance), state: filledState(spec, record), children: seenChildren(instance), facts: factsOf(instance, record.tool) };
  runs(spec.name, "view");
  try {
    // Its handlers get what `tick` gets.
    const context = contextOf(instance, spec, record, { time: now, transport }, {});
    say({ type: "tree", instance, tree: views.host.run(instance, spec.view, self, context) });
  } catch (error) {
    say({ type: "tree", instance, error: String(error) });
  }
}

const nameOf = (instance: string) => instance.slice(instance.lastIndexOf("/") + 1);
/** What the window told of the shown instances besides their records. */
let known: { facts: Record<string, Facts>; catalog: unknown[]; keys: Record<string, string | null> } = { facts: {}, catalog: [], keys: {} };
/** The facts of `id` as the window told them, or its title alone. */
const factsOf = (id: string, tool: string): Facts => known.facts[id] ?? kept(`${id} facts`, [tool], () => ({ title: sdk.host.tools.get(tool)?.title ?? nameOf(id), fields: [] }));
/** `children` of `parent` with their facts, and their children's at any depth, as views and actions get them. */
function seen(parent: string, children: readonly OwnedChild[]): OwnedChild[] {
  const list = children.map((child) => {
    const id = `${parent}/${child.name}`;
    const [facts, below] = [factsOf(id, child.tool), seen(id, child.children)];
    return kept(`${id} seen`, [child, facts, below], () => ({ ...child, facts, children: below }) as OwnedChild);
  });
  return kept(`${parent} seen list`, list, () => list);
}

/**
 * What was made last under `key`, while it is made of the same `parts`, else what `make` makes:
 * a child that did not change is the same object in every run, so a `memo` on it holds.
 */
const lastMade = new Map<string, { parts: readonly unknown[]; value: unknown }>();
function kept<T>(key: string, parts: readonly unknown[], make: () => T): T {
  const last = lastMade.get(key);
  if (last && last.parts.length === parts.length && last.parts.every((part, at) => part === parts[at])) return last.value as T;
  const value = make();
  lastMade.set(key, { parts, value });
  return value;
}
const seenChildren = (instance: string) => seen(instance, childrenOf(instance));
/** The key the action `name` of `tool` has now: as the window told, none included, else its first own key. */
const shortcutOf = (tool: string) => (name: string) => {
  const id = `${tool}.${name}`;
  return id in known.keys ? known.keys[id] : (sdk.host.tools.get(tool)?.actions?.[name]?.keys?.[0] ?? null);
};

/** The shown instances whose tools have no view, and the project's view of such a card. */
let carded = new Set<string>();
let genericCard: unknown;
/** The project's cards of built-in tools, by tool name, from `ui/blocks/cards/<tool>.tsx`. */
const toolCards = new Map<string, unknown>();
/** The file of the card that draws `instance` (its file runs that code, and is blamed for it), or `undefined` when no card does. */
const cardFile = (instance: string) => {
  if (!carded.has(instance)) return undefined;
  const tool = recordOf(instance)?.tool ?? "";
  return toolCards.has(tool) ? `${CARDS}/${tool}.tsx` : GENERIC_CARD;
};
/** The numbers a lane moves of each shown instance, by name as `held` takes it. */
let heldFields: Record<string, string[]> = {};
const heldIn = (instance: string) => (name: string) => heldFields[instance]?.includes(name) ?? false;
/** As `GENERIC_CARD` of the runtime names it, which loads it with the tools. */
const GENERIC_CARD = "ui/blocks/generic-card.tsx";
const CARDS = "ui/blocks/cards";

/** The card of an instance whose tool has no view: the project's card of its tool, else the generic card. */
function drawCard(instance: string, record: Entry) {
  const facts = factsOf(instance, record.tool);
  const defaults = Object.fromEntries(facts.fields.filter((field) => "default" in field).map((field) => [field.name, field.default]));
  const fill = (entry: Entry) => frozen({ ...defaults, ...(entry.state as object) });
  const self = { name: nameOf(instance), state: fill(record), children: [], facts };
  const key = `${instance} card`;
  if (!memories.has(key)) memories.set(key, {});
  // A card gets what a tool's view gets.
  const context = { ...performer(instance), memory: memories.get(key), clock: clockFor(instance), time: now, update: updater(instance, defaults), held: heldIn(instance), select: selector(instance), addOffer: offerAdder(instance), openWindow: (path: string) => say({ type: "open_window", instance, path }), show: shower(instance), shortcut: () => null, ...askers(instance) };
  Object.defineProperties(context, { ...newest(instance, fill, record), children: { enumerable: true, value: [] } });
  runs(cardFile(instance)!, "card");
  try {
    say({ type: "tree", instance, tree: views.host.run(instance, toolCards.get(record.tool) ?? genericCard, self, context) });
  } catch (error) {
    say({ type: "tree", instance, error: String(error) });
  }
}

function show(instances: string[]) {
  const asked = new Set(instances);
  for (const instance of shown) {
    if (!asked.has(instance)) {
      shown.delete(instance);
      views.host.forget(instance);
    }
  }
  for (const instance of asked) {
    if (!shown.has(instance)) {
      shown.add(instance);
      drawNext(instance);
    }
  }
}

/** The instance whose view runs an action now: what it changes is one step of undo, which ends with `acted`. */
let acting: string | undefined;

/** An action on a shown view; it runs again after, also when nothing changed, as its memory may have. */
function act({ instance, key, action, value }: Extract<Request, { type: "act" }>) {
  runs(cardFile(instance) ?? recordOf(instance)?.tool ?? "", `${action} on ${key}`);
  let error: string | undefined;
  acting = instance;
  try {
    views.host.perform(instance, key, action, value);
  } catch (thrown) {
    error = thrown instanceof Error ? thrown.message : String(thrown);
  } finally {
    acting = undefined;
  }
  say({ type: "acted", ...(error === undefined ? {} : { error }) });
  if (shown.has(instance)) drawNext(instance);
}

/** The record of `id` as its code knows it: as told, and with the edits not heard on top when `latest`. */
function recordOf(id: string, latest = true): Entry | undefined {
  let entry = told.get(id);
  for (const { changes } of latest ? unheard : []) {
    for (const change of changes) {
      if (change.id !== id) {
        continue;
      }
      if (change.type === "fields") {
        const state: Record<string, unknown> = { ...(entry?.state as object) };
        change.removed.forEach((name) => delete state[name]);
        entry = entry && { tool: entry.tool, state: Object.assign(state, change.set) };
      } else {
        entry = change.type === "record" ? { tool: change.tool, state: change.state } : undefined;
      }
    }
  }
  return entry;
}

/** The children of `id` in name order, as a behaviour gets them. */
function childrenOf(id: string, latest = true): OwnedChild[] {
  const known = kids.get(id);
  // As told: kept until a child changes, so a parent of a thousand takes asks for none of them
  // again, and an edit of one clip makes again only the lists it is in.
  if (known && (!latest || !editedBelow(id))) {
    return (known.told ??= listed(id, known.names, false));
  }
  return listed(id, known?.names ?? new Set(), latest);
}

/** Whether an edit the runtime has not said it heard changes a record inside `id`. */
function editedBelow(id: string): boolean {
  const inside = `${id}/`;
  return unheard.some(({ changes }) => changes.some((change) => change.id.startsWith(inside)));
}

function listed(id: string, told: Set<string>, latest: boolean): OwnedChild[] {
  const names = new Set(told);
  for (const { changes } of latest ? unheard : []) {
    for (const change of changes) {
      const [parent, name] = parentOf(change.id) ?? [];
      if (parent === id && name !== undefined && change.type !== "fields") {
        change.type === "record" ? names.add(name) : names.delete(name);
      }
    }
  }
  const list = [...names].sort().flatMap((name) => {
    const child = `${id}/${name}`;
    const entry = recordOf(child, latest);
    if (!entry) return [];
    const children = childrenOf(child, latest);
    return [kept(`${child} record`, [entry, children], () => frozen({ name, tool: entry.tool, state: filledOf(entry), children } as OwnedChild))];
  });
  return kept(`${id} records`, list, () => frozen(list));
}

/** The `state` of a record as the code of its tool gets it: every field, at its default where left out. */
function filledState(spec: Spec, entry: Entry): State {
  return frozen(filledObject(spec.state, entry.state));
}

/**
 * The state of a child as code gets it: every field that has a default at it, as the code of its
 * tool gets its own, so a view or a change reads `clip.state.start` with no `?? 0`.
 */
function filledOf(child: { tool: string; state: unknown }): unknown {
  const spec = sdk.host.tools.get(child.tool);
  if (spec) return filledObject(spec.state, child.state);
  const defaults = (sdk as { defaultStates?: Record<string, object> }).defaultStates?.[child.tool];
  return defaults && isObject(child.state) ? deepFilled(defaults, child.state) : child.state;
}

/** `state` with what it leaves out of `defaults` filled in, also inside its objects, as Rust reads it. */
function deepFilled(defaults: object, state: Record<string, unknown>): Record<string, unknown> {
  const filled: Record<string, unknown> = structuredClone(defaults) as Record<string, unknown>;
  for (const [name, value] of Object.entries(state)) {
    const fallback = filled[name];
    filled[name] = isObject(value) && isObject(fallback) ? deepFilled(fallback, value) : value;
  }
  return filled;
}

/** What a change did with a child it got: whether it read its state, and its children as it left them. */
type Drafted = { untouched: () => boolean; children: () => [OwnedChild[], unknown] | [] };
const drafts = new WeakMap<object, Drafted>();

/**
 * A copy of a child for a change to edit, whose state, and children, are copied when first
 * read. Its name stays: it is the id of the record, which never changes.
 */
function draft(child: OwnedChild): OwnedChild {
  let state: unknown;
  let copied = false;
  let children: [OwnedChild[], unknown] | [] = [];
  const copy = {} as OwnedChild;
  Object.defineProperties(copy, {
    name: {
      enumerable: true,
      get: () => child.name,
      set() {
        throw new TypeError(`the child ${child.name} keeps its name, the id of its record: keep a title in a field`);
      },
    },
    tool: { enumerable: true, writable: true, value: child.tool },
    state: {
      enumerable: true,
      get() {
        if (!copied) {
          state = structuredClone(filledOf(child));
          copied = true;
        }
        return state;
      },
      set(value) {
        state = value;
        copied = true;
      },
    },
    children: {
      enumerable: true,
      get() {
        if (children.length === 0) children = [child.children, child.children.map(draft)];
        return children[1];
      },
      set(value) {
        children = [child.children, value];
      },
    },
  });
  drafts.set(copy, { untouched: () => !copied, children: () => children });
  return copy;
}

/**
 * A copy of what code put in a record, as JSON: a record holds JSON, and a value read from
 * `memory` or another store is a proxy, which `structuredClone` refuses.
 */
function plain<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/** The fields `after` sets and leaves out against `before`, or `null` when it is the same. */
function fieldsChanged(before: unknown, after: unknown, what: string) {
  if (!isObject(after)) {
    throw new TypeError(`${what} is ${JSON.stringify(after)}: a state is an object of fields`);
  }
  const was = isObject(before) ? before : {};
  const set: Record<string, unknown> = {};
  const removed: string[] = [];
  for (const [name, value] of Object.entries(after)) {
    if (value === undefined) {
      if (name in was) removed.push(name);
    } else if (!(name in was) || JSON.stringify(value) !== JSON.stringify(was[name])) {
      set[name] = value;
    }
  }
  removed.push(...Object.keys(was).filter((name) => !(name in after)));
  return Object.keys(set).length > 0 || removed.length > 0 ? { set: plain(set), removed } : null;
}

/**
 * What a change made of the record of `instance`, `before` and now `state`, and of its children,
 * `edited` as they were `before` when it read them, at any depth.
 */
function changesOf(
  instance: string,
  [before, state]: [State, unknown],
  [children, edited]: [OwnedChild[], unknown] | [],
): Change[] {
  const changes: Change[] = [];
  const own = fieldsChanged(before, state, "state");
  if (own) {
    changes.push({ type: "fields", id: instance, ...own });
  }
  if (children !== undefined) {
    childChanges(instance, children, edited, changes);
  }
  return changes;
}

/** A child of a list a change left, checked: its name, its tool and its state. */
function childOf(child: unknown): OwnedChild {
  if (!isObject(child) || typeof child.name !== "string" || typeof child.tool !== "string") {
    throw new TypeError(`a child is { name, tool, state }, not ${JSON.stringify(child)}`);
  }
  if (child.name.includes("/")) {
    throw new Error(`the child ${child.name} is named with a \`/\`: a grandchild goes in the \`children\` of its parent`);
  }
  return child as OwnedChild;
}

/** What `edited`, the children of `parent` a change left, changes of `children`, as they were. */
function childChanges(parent: string, children: OwnedChild[], edited: unknown, changes: Change[]) {
  if (!Array.isArray(edited)) {
    throw new TypeError("children is a list of { name, tool, state }");
  }
  const old = new Map(children.map((child) => [child.name, child]));
  const kept = new Set<string>();
  for (const each of edited as unknown[]) {
    const child = childOf(each);
    const { name, tool } = child;
    if (kept.has(name)) {
      throw new Error(`two children are named ${name}`);
    }
    kept.add(name);
    const id = `${parent}/${name}`;
    const was = old.get(name);
    if (!was || was.tool !== tool) {
      created(id, child, changes);
      continue;
    }
    const drafted = drafts.get(child);
    if (!drafted?.untouched()) {
      // Against its defaults too: a field the file leaves out and the change did not touch stays out.
      const fields = fieldsChanged(filledOf(was), child.state, `the state of ${name}`);
      if (fields) {
        changes.push({ type: "fields", id, ...fields });
      }
    }
    // A draft's children, when the change read them; the children of a plain object as they are.
    const [owned, changed]: [OwnedChild[] | undefined, unknown] | [] = drafted ? drafted.children() : "children" in child ? [was.children ?? [], child.children] : [];
    if (owned) {
      childChanges(id, owned, changed, changes);
    }
  }
  for (const name of old.keys()) {
    if (!kept.has(name)) {
      // What it owns goes with it.
      changes.push({ type: "delete", id: `${parent}/${name}` });
    }
  }
}

/** A new record at `id` and the children it comes with, parents first. */
function created(id: string, child: OwnedChild, changes: Change[]) {
  changes.push({ type: "record", id, tool: child.tool, state: plain(child.state ?? {}) });
  const inner: unknown = (child as { children?: unknown }).children;
  if (inner === undefined) return;
  if (!Array.isArray(inner)) {
    throw new TypeError(`the children of ${id} are a list of { name, tool, state }`);
  }
  const names = new Set<string>();
  for (const each of inner) {
    const grandchild = childOf(each);
    if (names.has(grandchild.name)) throw new Error(`two children of ${id} are named ${grandchild.name}`);
    names.add(grandchild.name);
    created(`${id}/${grandchild.name}`, grandchild, changes);
  }
}

/**
 * What changes the record of an instance and those of its children, each time on the latest the
 * code knows: also after an `await`, while the instance is there.
 */
function updater(instance: string, defaults?: Record<string, unknown>) {
  return (label: string, change: (part: Part<State>) => void) => {
    if (typeof label !== "string") {
      throw new TypeError(`the label of an update is ${JSON.stringify(label)}: give text, such as "Save the take"`);
    }
    const own = recordOf(instance);
    const spec = sdk.host.tools.get(own?.tool ?? "");
    if (!own || (!spec && !defaults)) {
      return;
    }
    // A tool with no code here, such as a built-in one, has its defaults from its generic card.
    const state = spec ? filledState(spec, own) : (frozen({ ...defaults, ...(own.state as object) }) as State);
    // The children only when the change reads them: an update of a parent of a thousand takes
    // that edits its own fields costs nothing for them.
    let children: [OwnedChild[], unknown] | [] = [];
    const part = { state: structuredClone(state) } as Part<State>;
    Object.defineProperty(part, "children", {
      enumerable: true,
      get() {
        if (children.length === 0) {
          const before = childrenOf(instance);
          children = [before, before.map(draft)];
        }
        return children[1];
      },
      set(value) {
        children = [children[0] ?? childrenOf(instance), value];
      },
    });
    change(part);
    const [before, after] = children as [OwnedChild[]?, unknown?];
    views.host.refuseHeld(heldFields[instance] ?? [], { state, children: before }, { state: part.state, children: after as object });
    const changes = changesOf(instance, [state, part.state], children);
    if (changes.length === 0) {
      return;
    }
    const wired = changes.map(({ id, ...change }) => (id === instance ? change : { ...change, child: id.slice(instance.length + 1) }));
    // Before it counts: a value JSON cannot hold, such as a cycle, throws here.
    const line = JSON.stringify({ type: "edit", instance, label, changes: wired, ...(acting === instance ? { gesture: true } : {}) }) + "\n";
    edits += 1;
    recordsChanged += 1;
    unheard.push({ number: edits, changes });
    writeSync(1, line);
  };
}

/**
 * What a view, `tick`, an action, `onKey` and `onNotes` of an instance get: its records as they
 * are when read, also by a handler a `memo` kept from an earlier run and after an `update`, and
 * its memory.
 */
function contextOf<Extra extends object>(
  instance: string,
  spec: Spec,
  record: Entry,
  { time }: Clock,
  extra: Extra,
) {
  const context = {
    ...extra,
    time,
    ...doers(instance, spec),
  };
  return Object.defineProperties(context, newest(instance, (entry) => filledState(spec, entry), record)) as typeof context & { state: State; children: OwnedChild[]; catalog: unknown[] };
}

/** How many times what code reads changed: the runtime told records or facts, or code sent an edit. */
let recordsChanged = 0;

/**
 * `state` and `children` of `instance`, with their facts, as they are when read, made again only
 * after the records or facts changed: children only when read, so code of a parent of a thousand
 * takes that does not costs nothing for them. `record` while the instance is gone.
 */
function newest(instance: string, fill: (entry: Entry) => unknown, record: Entry): PropertyDescriptorMap {
  let state: [number, unknown] | undefined;
  let children: [number, OwnedChild[]] | undefined;
  return {
    catalog: { enumerable: true, get: () => known.catalog },
    state: { enumerable: true, get: () => (state?.[0] === recordsChanged ? state : (state = [recordsChanged, fill(recordOf(instance) ?? record)]))[1] },
    children: { enumerable: true, get: () => (children?.[0] === recordsChanged ? children : (children = [recordsChanged, seenChildren(instance)]))[1] },
  };
}

/** What every piece of code of an instance can do, in a view or in `tick`, an action, `onKey`, `onNotes` and `onTake`. */
function doers(instance: string, spec: Spec) {
  return {
    memory: memoryOf(instance, spec),
    clock: clockFor(instance),
    update: updater(instance),
    held: heldIn(instance),
    select: selector(instance),
    addOffer: offerAdder(instance),
    openWindow: (path: string) => say({ type: "open_window", instance, path }),
    show: shower(instance),
    shortcut: shortcutOf(spec.name),
    ...askers(instance),
    ...performer(instance),
  };
}

/** `select`: what the composer works on, a child by its path, which the window plays live notes into. */
function selector(instance: string) {
  return (path: string | null, { span }: { span?: { from: number; to: number } } = {}) => {
    const range = span && [wholeTick(span.from, "span from"), wholeTick(span.to, "span to")];
    say({ type: "select", instance, path, span: range ?? null });
  };
}

/** `show`: the window shows a range of a plane of the view of `instance`. */
function shower(instance: string) {
  return (key: string, { x, y }: { x?: [number, number]; y?: [number, number] }) => {
    const range = (axis: [number, number] | undefined, what: string) => axis && [finite(axis[0], `${what} from`), finite(axis[1], `${what} to`)];
    say({ type: "show_range", instance, key, x: range(x, "x") ?? null, y: range(y, "y") ?? null });
  };
}

/** Questions to the window, by number, which its answers settle, and the instance that asked: `openFile`, `render`, `exportNotes`, `confirm`. */
const asked = new Map<number, { resolve: (answer: unknown) => void; instance: string }>();
let questions = Number(bunNumber) * 2 ** 32;
/** Questions asked since the last ops, which go after them: a menu a new dialog opens belongs to it. */
const unsaid: object[] = [];
function ask<Answer>(instance: string, message: { type: string } & Record<string, unknown>): Promise<Answer> {
  const question = ++questions;
  unsaid.push({ ...message, question });
  later();
  return new Promise((resolve) => asked.set(question, { resolve: resolve as (answer: unknown) => void, instance }));
}

/** What a tool asks the window, for its instance. */
function askers(instance: string) {
  return {
    /** The runtime opens the file and copies it under `assets/audio/`. */
    openFile: ({ title = "Choose a file" }: { title?: string } = {}) => ask<string | null>(instance, { type: "open_file", title }),
    render: (path: string, { from, to, tail = true, name = path }: { from: number; to: number; tail?: boolean; name?: string }) =>
      ask<string | null>(instance, { type: "render", instance, path, span: [wholeTick(from, "render from"), wholeTick(to, "render to")], tail, name }),
    confirm: ({ message, detail, confirm }: { message: string; detail?: string; confirm: string }) => ask<boolean>(instance, { type: "confirm", message, detail, confirm }),
    exportNotes: (path: string, { name }: { name: string }) => ask<boolean>(instance, { type: "export_notes", instance, path, name }),
  };
}

/** `addOffer`: the window writes the offer's record into the child, as one edit. */
function offerAdder(instance: string) {
  return (key: string, path: string, label: string) => say({ type: "add_offer", instance, path, key, label, ...(acting === instance ? { gesture: true } : {}) });
}

// ---------------------------------------------------------------------------------------------
// The clock code reads, and what it asks the window.

/** Where the piece plays as last told: what moves on every frame, which no binding follows. */
let transport: Transport = { playing: false, tick: 0, bar: 1, beat: 1, bpm: 120, repeat: null, click: false, keys: false, automation: false };
/** What of the clock bindings follow, one for every tool. */
const following = ({ playing: false, repeat: null as { from: number; to: number } | null, click: false, keys: false, automation: false });
let flushing = false;
const song: Song = { tempo_map: { time_signatures: [{ signature: "4/4", bars: 1 }], tempo_changes: [{ tick: 0, bpm: 120 }] }, sections: [] };

/** A tick as the window takes it: a whole number from 0. */
function wholeTick(tick: number, what: string): number {
  if (!Number.isFinite(tick) || tick < 0) throw new Error(`${what} ${tick} is not a tick from 0 on`);
  return Math.round(tick);
}

/** The clock as the code of `instance` gets it: what bindings follow, and what moves it. */
function clockFor(instance: string): SdkClock {
  const moving = (name: "tick" | "bar" | "beat" | "bpm") => transport[name];
  const op = (message: object) => say({ type: "transport", ...message });
  const follows = <T>(into: Set<string>, value: T) => {
    if (drawing === instance) into.add(instance);
    return value;
  };
  return {
    get playing() { return follows(followsClock.playing, following.playing); },
    get repeat() { return follows(followsClock.repeat, following.repeat); },
    get click() { return follows(followsClock.click, following.click); },
    get computerKeys() { return follows(followsClock.keys, following.keys); },
    get writeAutomation() { return follows(followsClock.automation, following.automation); },
    get tempo_map() { return follows(followsSong, song.tempo_map); },
    get sections() { return follows(followsSong, song.sections); },
    get tick() { return moving("tick"); },
    get bar() { return moving("bar"); },
    get beat() { return moving("beat"); },
    get bpm() { return moving("bpm"); },
    play: () => op({ op: "play" }),
    pause: () => op({ op: "pause" }),
    stop: () => op({ op: "stop" }),
    jump: (to, { atTick } = {}) => op({ op: "jump", to: wholeTick(to, "jump to"), at: atTick === undefined ? null : wholeTick(atTick, "jump atTick") }),
    setRepeat: (range) => op({ op: "repeat", range: range && [wholeTick(range.from, "repeat from"), wholeTick(range.to, "repeat to")] }),
    setClick: (on) => op({ op: "click", on }),
    setComputerKeys: (on) => op({ op: "keys", on }),
    setWriteAutomation: (on) => op({ op: "automation", on }),
    update: (label, change) => {
      const draft = structuredClone({ tempo_map: song.tempo_map, sections: song.sections });
      change(draft);
      // Before it counts: a value JSON cannot hold throws here.
      const line = JSON.stringify({ type: "song_edit", instance, label, ...draft, ...(acting === instance ? { gesture: true } : {}) }) + "\n";
      writeSync(1, line);
      // What its views show follows at once, before the runtime answers.
      Object.assign(song, draft);
      later();
    },
  };
}

function setClock({ time, transport: next }: Clock) {
  now = time;
  transport = next;
  const repeat = next.repeat && { from: next.repeat[0], to: next.repeat[1] };
  const followed = { playing: next.playing, repeat, click: next.click, keys: next.keys, automation: next.automation };
  const moved = (Object.keys(followed) as (keyof typeof followed)[]).filter((name) => JSON.stringify(followed[name]) !== JSON.stringify(following[name]));
  if (moved.length === 0) return;
  const views = new Set(moved.flatMap((name) => [...followsClock[name]]));
  Object.assign(following, followed);
  [...views].filter((id) => shown.has(id)).forEach(drawNext);
}

/** The views that read each part of the clock that bindings follow, and those that read the song, when they last ran. */
const followsClock: Record<keyof typeof following, Set<string>> = { playing: new Set(), repeat: new Set(), click: new Set(), keys: new Set(), automation: new Set() };
const followsSong = new Set<string>();
/** The instance whose view runs now. */
let drawing: string | undefined;

/** Sends the ops at once when code changed something outside a request, such as after an `await`. */
function later() {
  if (!flushing) {
    flushing = true;
    queueMicrotask(flushOps);
  }
}

/** Sends the questions code asked since. */
function flushOps() {
  flushing = false;
  for (const message of unsaid.splice(0)) {
    say(message);
  }
}

/** `value` of `field` as the window gives it: at its default when left out, and so each item. */
function filled(field: Field, value: unknown): unknown {
  const given = value ?? defaultOf(field);
  if (field.kind !== "list" || !Array.isArray(given)) {
    return given;
  }
  const item = field.item;
  return given.map((each) => (item.kind === "object" ? filledObject(item.fields, each) : filled(item, each)));
}

function filledObject(fields: Fields, value: unknown): State {
  const object = (value ?? {}) as Record<string, unknown>;
  return Object.fromEntries(Object.entries(fields).map(([name, field]) => [name, filled(field, object[name])])) as State;
}

function defaultOf(field: Field): unknown {
  switch (field.kind) {
    case "file":
      return "";
    case "list":
      return field.default ?? [];
    case "text":
      return field.default ?? "";
    case "number":
      return field.default ?? field.min;
    default:
      return field.default;
  }
}

/** The ops of a tool's view at its defaults, after one tick: a check without a window. */
function checkView(spec: Spec): unknown {
  const memory = freshMemory(spec);
  const performs = { play() {}, send() {}, update() {}, held: () => false, select() {}, openFile: async () => null, render: async () => null, confirm: async () => false, addOffer() {}, openWindow() {}, show() {}, catalog: [], shortcut: () => null };
  const state = frozen(filledObject(spec.state, {}));
  // What the check plays on the clock goes nowhere.
  const clock = { ...clockFor(""), play() {}, pause() {}, stop() {}, jump() {}, setRepeat() {}, setClick() {}, setComputerKeys() {}, update() {} };
  const context = { state, children: [], memory, time: 0, clock, ...performs };
  try {
    spec.tick?.({ ...context, dt: 1 / 30 } as never);
  } catch (error) {
    throw new Error(`its tick fails: ${error}`);
  }
  const checked = `check ${spec.name}`;
  if (spec.view) views.host.run(checked, spec.view, { name: spec.name, state, children: [], facts: { title: spec.title, fields: [] } }, context);
  views.host.forget(checked);
  return null;
}

/**
 * Runs the code of each instance that `call` picks, with what all its code gets; its cards
 * follow what it changed. `what` names that code in a problem.
 */
function run<Extra extends object>(
  // Only the clock: a key or notes request has an `instance` of its own, which the cards would draw.
  { time, transport }: Clock,
  instances: Played[],
  what: string,
  extra: Extra,
  call: (spec: Spec, tool: Context<State, Inputs, unknown> & Extra) => unknown,
) {
  const clock = { time, transport };
  setClock(clock);
  for (const { instance } of instances) {
    const record = recordOf(instance);
    const spec = sdk.host.tools.get(record?.tool ?? "");
    if (!spec || !record) {
      continue;
    }
    const failed = (error: unknown) => fail(spec.name, `its ${what} failed: ${error}`);
    try {
      runs(spec.name, what);
      const context = contextOf(instance, spec, record, clock, extra) as unknown as Context<State, Inputs, unknown> & Extra;
      caught(call(spec, context), failed);
    } catch (error) {
      failed(error);
    }
  }
}

/** Hears the failure of code that went on after an `await`. */
function caught(result: unknown, failed: (error: unknown) => void) {
  if (result instanceof Promise) {
    result.catch(failed);
  }
}

/** Does what the window said. The instance whose code goes on after an answer, which draws again once it ran. */
function handle(request: Request): string | undefined {
  switch (request.type) {
    case "records":
      hear(request);
      break;
    case "clock":
      setClock(request);
      break;
    case "song":
      Object.assign(song, { tempo_map: request.tempo_map, sections: request.sections });
      [...followsSong].filter((id) => shown.has(id)).forEach(drawNext);
      break;
    case "frame":
      run(request, request.instances, "tick", { dt: request.dt }, (spec, tool) => spec.tick?.(tool));
      say({ type: "framed" });
      break;
    case "key": {
      const { key: name, down, shift, alt, cmd, ctrl } = request;
      const key = { key: name, down, shift, alt, cmd, ctrl };
      run(request, [request.instance], "onKey", {}, (spec, tool) => spec.onKey?.(tool, key));
      break;
    }
    case "action": {
      // Until one does not say `false`. One that fails, or whose tool is gone, ends it too. What
      // they change is one step of undo, which ends with `acted`.
      const { instance } = request.instance;
      let passed = true;
      acting = instance;
      try {
        for (const name of request.actions) {
          if (!passed) break;
          passed = false;
          run(request, [request.instance], `action ${name}`, {}, (spec, tool) => {
            const result = spec.actions?.[name]?.run(tool);
            passed = result === false;
            return result;
          });
        }
      } finally {
        acting = undefined;
      }
      say({ type: "acted" });
      if (shown.has(instance)) drawNext(instance);
      break;
    }
    case "take": {
      const { recorder, name, tool: kind, state } = request;
      run(request, [request.instance], "onTake", {}, (spec, tool) => spec.onTake?.(tool, { recorder, name, tool: kind, state } as Take));
      break;
    }
    case "notes":
      run(request, [request.instance], "onNotes", {}, (spec, tool) =>
        Promise.all(request.events.map((event) => spec.onNotes?.(tool, event))),
      );
      break;
    case "behaviour":
      declare(request);
      break;
    case "show": {
      const sent = { facts: request.facts ?? {}, catalog: request.catalog ?? [], keys: request.keys ?? {} };
      // A view whose card, held numbers or facts changed, its own or those of what it holds, draws again; every one when the catalog or the keys did.
      const all = JSON.stringify([sent.catalog, sent.keys]) !== JSON.stringify([known.catalog, known.keys]);
      const same = (before: unknown, after: unknown) => JSON.stringify(before) === JSON.stringify(after);
      const factsMoved = Object.keys({ ...known.facts, ...sent.facts }).filter((id) => !same(known.facts[id], sent.facts[id]));
      const changed = request.instances.filter(
        (id) =>
          shown.has(id) &&
          (all ||
            carded.has(id) !== request.cards.includes(id) ||
            !same(heldFields[id], request.held[id]) ||
            factsMoved.some((moved) => moved === id || moved.startsWith(`${id}/`))),
      );
      carded = new Set(request.cards);
      heldFields = request.held;
      if (factsMoved.length > 0) recordsChanged += 1;
      // Facts that did not move stay the objects they were, so the children made of them do.
      const moved = new Set(factsMoved);
      for (const id of Object.keys(sent.facts)) if (!moved.has(id)) sent.facts[id] = known.facts[id]!;
      known = sent;
      show(request.instances);
      changed.forEach(drawNext);
      break;
    }
    case "act":
      act(request);
      break;
    case "answer": {
      const question = asked.get(request.question);
      asked.delete(request.question);
      question?.resolve(request.answer);
      return question?.instance;
    }
  }
  return undefined;
}


await load();

for await (const line of console) {
  // Bun gives the end of the input after the last newline as one more, empty, line.
  if (line === "") {
    continue;
  }
  try {
    const answered = handle(JSON.parse(line) as Request);
    if (answered !== undefined) {
      // Its code goes on in the microtasks before this; whatever it awaits next waits on.
      await new Promise((resolve) => setTimeout(resolve, 0));
      if (shown.has(answered)) drawNext(answered);
    }
  } finally {
    flushOps();
    // Bun does one thing at a time, so the runtime knows what hangs: the oldest not done.
    say({ type: "done" });
  }
}
// The runtime closed stdin: it quit.
process.exit(0);
