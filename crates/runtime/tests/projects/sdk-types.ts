// Types an agent's tool must get right. A test checks this file with `bun check` next to the
// SDK of the built-in tools: every `@ts-expect-error` line must be an error, and nothing else.
import { h, input, number, list, notes, output, rise, text, tool, bang, type Declaration, type HeardEvent, type TimelineState } from "./sdk";

const rows = list({ name: text(), steps: number({ min: 0, max: 8 }) }, { default: [{ name: "a" }] });

// @ts-expect-error: an item has no field `title`.
list({ name: text() }, { default: [{ title: "x" }] });

tool({
  name: "types",
  title: "Types",
  when: "Never",
  doc: "Types.",
  state: { rows, plain: list(text()) },
  behaviour: ({ state }) => {
    const name: string = state.rows[0]!.name;
    const steps: number = state.rows[0]!.steps;
    const plain: string = state.plain[0]!;
    // @ts-expect-error: an item has no field `title`.
    state.rows[0]!.title;
    const connections = [{ from: { child: name + steps + plain }, to: { port: "audio" as const } }];
    return { connections };
  },
});

tool({
  name: "typo",
  title: "Typo",
  when: "Never",
  doc: "Typo.",
  state: {},
  behaviour: (): Declaration => ({
    connections: [],
    // @ts-expect-error: a misspelled key, which only the annotated return type catches.
    player: {},
  }),
});

tool({
  name: "cables",
  title: "Cables",
  when: "Never",
  doc: "Cables.",
  state: {},
  behaviour: (): Declaration => ({
    connections: [
      { from: { child: "lfo", number: "wave" }, to: { child: "filter", number: "cutoff_hz" } },
      // @ts-expect-error: a number goes into a number, not into a sound.
      { from: { child: "lfo", number: "wave" }, to: { child: "filter" } },
      // @ts-expect-error: and a sound not into a number.
      { from: { child: "lfo" }, to: { child: "filter", number: "cutoff_hz" } },
      { from: { child: "ears", notes: "hits" }, to: { child: "synth", notes: "notes" } },
      { from: { child: "ears", bang: "onset" }, to: { child: "drop", bang: "hit" } },
      { from: { child: "lfo", number: "wave" }, to: { child: "drop", number: "rate" } },
      // @ts-expect-error: notes go into notes, not into a number.
      { from: { child: "ears", notes: "hits" }, to: { child: "filter", number: "cutoff_hz" } },
      // @ts-expect-error: a bang goes into a bang, and moves no number.
      { from: { child: "ears", bang: "onset" }, to: { child: "drop", number: "rate" } },
      // @ts-expect-error: and notes are no sound.
      { from: { child: "ears", notes: "hits" }, to: { child: "fx" } },
      // @ts-expect-error: a sound moves no number.
      { from: { child: "lfo" }, to: { child: "drop", number: "rate" } },
      // @ts-expect-error: and sends no bang.
      { from: { child: "lfo" }, to: { child: "drop", bang: "hit" } },
      // @ts-expect-error: one port has the key of one kind.
      { from: { child: "ears", notes: "hits", number: "level" }, to: { child: "synth", notes: "notes" } },
    ],
  }),
});

tool({
  name: "own",
  title: "Own",
  when: "Never",
  doc: "Its own ports are ends.",
  state: {},
  behaviour: (): Declaration => ({
    connections: [
      { from: { notes: "notes" }, to: { child: "keys", notes: "notes" } },
      { from: { port: "audio" }, to: { child: "vocoder" } },
      { from: { child: "keys" }, to: { port: "audio" } },
      { from: { child: "lfo", number: "wave" }, to: { number: "wave" } },
      { from: { child: "ears", bang: "onset" }, to: { bang: "onset" } },
      { from: { child: "seq", notes: "notes" }, to: { notes: "melody" } },
      // @ts-expect-error: its own sound output is `audio`.
      { from: { child: "keys" }, to: { port: "send" } },
      // @ts-expect-error: its own notes are `notes`.
      { from: { notes: "keys" }, to: { child: "keys", notes: "notes" } },
      // @ts-expect-error: its own notes are no sound.
      { from: { notes: "notes" }, to: { port: "audio" } },
      // @ts-expect-error: a number goes out as a number.
      { from: { child: "lfo", number: "wave" }, to: { bang: "wave" } },
      // @ts-expect-error: a parent takes no number of its own from outside.
      { from: { number: "wave" }, to: { child: "filter", number: "cutoff_hz" } },
    ],
  }),
});

tool({
  name: "conductor",
  title: "Conductor",
  when: "Never",
  doc: "Plays its children.",
  state: {},
  inputs: { hit: bang() },
  behaviour: (): Declaration => ({ connections: [] }),
  tick: ({ play, send }) => {
    send("hit");
    send("hit", { at: 1 });
    send({ child: "lfo", number: "rate" }, 2);
    send({ child: "drums", bang: "kick" }, { at: 1 });
    play(60, { child: "synth", hold: true });
    send({ child: "synth", notes: "notes" }, { type: "noteOff", pitch: 60 });
    send({ child: "synth", notes: "notes" }, { type: "cc", controller: 64, value: 1 }, { at: 1 });
    send("notes", { type: "bend", value: -0.5 });
    // @ts-expect-error: a misspelled event.
    send("notes", { type: "noteoff", pitch: 60 });
    // @ts-expect-error: a note on needs its velocity.
    send("notes", { type: "noteOn", pitch: 60 });
    // @ts-expect-error: a key is let go of with a `noteOff`.
    send("notes", { type: "noteOff", pitch: 60, velocity: 0 });
    // @ts-expect-error: a bang takes no number.
    send("hit", 1);
    // @ts-expect-error: and an input it does not have.
    send("hits");
    // @ts-expect-error: a number input of a child takes a number.
    send({ child: "lfo", number: "rate" });
  },
});

tool({
  name: "recorder",
  title: "Recorder",
  when: "Never",
  doc: "Keeps what it hears and plays it back.",
  voices: 8,
  state: {},
  memory: () => ({ heard: [] as HeardEvent[] }),
  onNotes: ({ memory, send }, event) => {
    memory.heard.push(event);
    send("notes", event, { at: event.at + 1 });
  },
  // @ts-expect-error: `release` is gone: a `noteOff` lets go.
  tick: ({ release }) => release(60),
});

tool({
  name: "old",
  title: "Old",
  when: "Never",
  doc: "Hears the keyboard the old way.",
  state: {},
  // @ts-expect-error: `onMidi` is `onNotes`.
  onMidi: () => {},
});

tool({
  name: "ears",
  title: "Ears",
  when: "Never",
  doc: "Sends what it hears.",
  state: {},
  sound: () => {
    output("level", input);
    output("onset", { bang: rise(input) });
    output("hits", { gate: input, pitch: 60 });
    // @ts-expect-error: notes need a pitch.
    output("broken", { gate: input });
    // @ts-expect-error: a bang or notes, not both.
    output("both", { bang: input, gate: input, pitch: 60 });
    return input;
  },
});

tool({
  name: "takes",
  title: "Takes",
  when: "Never",
  doc: "Saves each take as a child, and edits its timeline.",
  state: { count: number({ min: 0, max: 100 }) },
  behaviour: (): Declaration => ({ connections: [] }),
  view: ({ time, clock, children }) => h("div", {}, () => `${children.length} at ${time} s, ${clock.playing ? "playing" : "stopped"}`),
  onNotes: async ({ update, children, state, clock }, event) => {
    const beats: number = (event.at * clock.bpm) / 60;
    // @ts-expect-error: the clock moves only through its calls.
    clock.tick = 0;
    // @ts-expect-error: a jump goes to a tick, not to a bar and beat.
    clock.jump({ bar: 2 });
    const taken: number = children.length;
    update("Save a take", ({ state, children }) => {
      state.count = taken + 1;
      children.push({ name: `take-${taken}`, tool: "take", state: { notes: [] } });
      const bass = children.find((child) => child.name === "bass")!.state as TimelineState;
      bass.notes?.push({ start: 0, length: 480, pitch: event.type === "noteOn" ? event.pitch : 60, velocity: 100 });
      // @ts-expect-error: a child's state is any tool's: say which it is.
      children[0]!.state.notes;
      // @ts-expect-error: a child names its tool.
      children.push({ name: "take", state: {} });
      // @ts-expect-error: a field it does not have.
      state.counts = 1;
      // @ts-expect-error: a child keeps its name, the id of its record.
      children[0]!.name = "renamed";
    });
    // @ts-expect-error: the record changes through `update`.
    state.count = 1;
    const code: number = await Bun.spawn(["sleep", "0"]).exited;
    await Bun.sleep(code);
    update("Name the take", (part) => {
      part.children = part.children.filter((child) => child.name !== "take-0");
    });
    // @ts-expect-error: `change` gets the record and its children, not the state alone.
    update("Old", (state) => { state.count = 1; });
  },
  tick: ({ update }) => update("Keep", ({ state }) => { state.count = 0; }),
});

tool({
  name: "take",
  title: "Take",
  when: "Never",
  doc: "A take.",
  state: { notes: notes() },
});

const parts = { bass: "timeline", fx: "filter", drop: "rain" } as const;

tool({
  name: "loops",
  title: "Loops",
  when: "Never",
  doc: "Names its children, so their ports and records are typed.",
  state: {},
  children: parts,
  behaviour: ({ children }): Declaration<typeof parts> => {
    const bass = children.find((child) => child.name === "bass");
    const length: number | undefined = bass?.state.notes?.[0]?.length;
    if (length === 0) return { connections: [] };
    return {
      connections: [
        { from: { child: "drop", number: "level" }, to: { child: "fx", number: "cutoff_hz" } },
        { from: { child: "drop", number: "level" }, to: { child: "bass", number: "launch" } },
        { from: { child: "bass", notes: "notes" }, to: { child: "drop", notes: "notes" } },
        { from: { child: "fx" }, to: { port: "audio" } },
        // @ts-expect-error: the filter has no number `cutoff`.
        { from: { child: "drop", number: "level" }, to: { child: "fx", number: "cutoff" } },
        // @ts-expect-error: and no number input.
        { from: { child: "drop", number: "level" }, to: { child: "fx", number: "launch" } },
        // @ts-expect-error: a child it does not name.
        { from: { child: "synth" }, to: { port: "audio" } },
      ],
    };
  },
  tick: ({ send, play, update }) => {
    send({ child: "bass", number: "launch" }, 1);
    // @ts-expect-error: the timeline's number input is `launch`.
    send({ child: "bass", number: "lunch" }, 1);
    // @ts-expect-error: a number field of a child is the composition: a cable moves it.
    send({ child: "fx", number: "cutoff_hz" }, 800);
    // @ts-expect-error: a child it does not name.
    play(60, { child: "synth" });
    update("Edit", ({ children }) => {
      for (const child of children) {
        if (child.name === "fx") child.state.type = "high_pass";
        // @ts-expect-error: a choice of a child is one of its options.
        if (child.name === "fx") child.state.type = "notches";
        // @ts-expect-error: a field the record of a timeline does not have.
        if (child.name === "bass") child.state.patterns = [];
      }
      children.push({ name: "fx", tool: "filter", state: { cutoff_hz: 500 } });
      // @ts-expect-error: the child `fx` is a filter.
      children.push({ name: "fx", tool: "delay", state: {} });
    });
  },
});

const loose = { bass: "timeline" };

tool({
  name: "loose",
  title: "Loose",
  when: "Never",
  doc: "Names its children without `as const`.",
  state: {},
  // @ts-expect-error: TypeScript widens each tool name to `string`, which checks nothing.
  children: loose,
  behaviour: (): Declaration => ({ connections: [] }),
});

tool({
  name: "inline",
  title: "Inline",
  when: "Never",
  doc: "Names its children in place, with a return type that names none.",
  state: {},
  // @ts-expect-error: a plain `Declaration` widens the tool names too.
  children: { bass: "timeline" },
  behaviour: (): Declaration => ({ connections: [] }),
});

const drone = tool({
  name: "drone",
  title: "Drone",
  when: "Never",
  doc: "A tool of the project that a parent names by what `tool` returns.",
  state: { pitch: number({ min: 0, max: 127, default: 60 }), name: text() },
  inputs: { swell: number({ min: 0, max: 1, default: 0 }), hit: bang() },
});
const other = tool({
  name: "other",
  title: "Other",
  when: "Never",
  doc: "A tool of the project with the same fields as the drone.",
  state: { pitch: number({ min: 0, max: 127, default: 60 }), name: text() },
  inputs: { swell: number({ min: 0, max: 1, default: 0 }), hit: bang() },
});
const band = { lead: drone, fx: "filter" } as const;

tool({
  name: "band",
  title: "Band",
  when: "Never",
  doc: "Names a tool of the project among its children, so its ports and record are typed.",
  state: {},
  children: band,
  behaviour: ({ children }): Declaration<typeof band> => {
    const pitch: number | undefined = children.find((child) => child.name === "lead")?.state.pitch;
    return {
      connections: [
        { from: { child: "lead", number: "anything" }, to: { child: "lead", number: "pitch" } },
        { from: { child: "lead", notes: "melody" }, to: { child: "lead", notes: "notes" } },
        { from: { child: "lead", bang: "onset" }, to: { child: "lead", bang: "hit" } },
        { from: { child: "lead", number: String(pitch) }, to: { child: "lead", number: "swell" } },
        { from: { child: "lead" }, to: { child: "fx" } },
        // @ts-expect-error: the drone has no number `pitc`.
        { from: { child: "lead", number: "level" }, to: { child: "lead", number: "pitc" } },
        // @ts-expect-error: its input of notes is `notes`.
        { from: { child: "lead", notes: "melody" }, to: { child: "lead", notes: "keys" } },
      ],
    };
  },
  tick: ({ send, update }) => {
    send({ child: "lead", number: "swell" }, 1);
    send({ child: "lead", bang: "hit" });
    // @ts-expect-error: the drone has no number input `swel`.
    send({ child: "lead", number: "swel" }, 1);
    // @ts-expect-error: and no bang `hits`.
    send({ child: "lead", bang: "hits" });
    // @ts-expect-error: its field `pitch` is the composition: a cable moves it, `send` does not.
    send({ child: "lead", number: "pitch" }, 60);
    update("Edit", ({ children }) => {
      for (const child of children) {
        if (child.name === "lead") child.state.pitch = 62;
        // @ts-expect-error: a field the drone does not have.
        if (child.name === "lead") child.state.pitches = 62;
      }
      // @ts-expect-error: the lead is a drone, not another tool with the same fields.
      children.push({ name: "lead", tool: other, state: { pitch: 60, name: "" } });
    });
  },
});

const counter = tool({
  name: "counter",
  title: "Counter",
  when: "Never",
  doc: "A whole number and a level.",
  state: { count: number({ min: 1, max: 16, default: 4, integer: true }), level: number({ min: 0, max: 1, default: 0 }) },
});
const counted = { counter, drop: "rain" } as const;

tool({
  name: "counting",
  title: "Counting",
  when: "Never",
  doc: "Moves the level of its counter.",
  state: {},
  children: counted,
  behaviour: (): Declaration<typeof counted> => ({
    connections: [
      { from: { child: "drop", number: "level" }, to: { child: "counter", number: "level" } },
      // @ts-expect-error: a whole number moves by no cable.
      { from: { child: "drop", number: "level" }, to: { child: "counter", number: "count" } },
    ],
  }),
});

tool({
  name: "keyed",
  title: "Keyed",
  when: "Never",
  doc: "Keys.",
  sound: () => 0,
  keys: ["delete", "cmd-c", "cmd-alt-left"],
  // @ts-expect-error: no key is called `del`, and shift is not a name of a key.
  globalKeys: ["del", "shift-a"],
  onKey: ({ clock }, key) => {
    // @ts-expect-error: no key is called `spcae`.
    if (key.key === "spcae") clock.play();
    // @ts-expect-error: a jump waits for a tick named `atTick`, not `at`, which is a time elsewhere.
    clock.jump(0, { at: 3840 });
    clock.jump(0, { atTick: 3840 });
  },
});

tool({
  name: "app-keyed",
  title: "App keyed",
  when: "Never",
  doc: "Keys the app keeps.",
  sound: () => 0,
  // @ts-expect-error: tab and cmd-z are the app's.
  globalKeys: ["tab", "cmd-z"],
});
