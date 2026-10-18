// Types an agent's tool must get right. A test checks this file with `bun check` next to the
// SDK of the built-in tools: every `@ts-expect-error` line must be an error, and nothing else.
import { devices, h, input, knob, list, live, notes, number, output, prefixed, rise, text, tool, trigger, type Declaration, type HeardEvent, type SequencerState } from "./sdk";

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
      { from: { child: "ears", bang: "onset" }, to: { child: "drop", trigger: "hit" } },
      { from: { child: "lfo", number: "wave" }, to: { child: "drop", live: "rate" } },
      // @ts-expect-error: notes go into notes, not into a number.
      { from: { child: "ears", notes: "hits" }, to: { child: "filter", number: "cutoff_hz" } },
      // @ts-expect-error: a bang fires a trigger, and moves no live control.
      { from: { child: "ears", bang: "onset" }, to: { child: "drop", live: "rate" } },
      // @ts-expect-error: and notes are no sound.
      { from: { child: "ears", notes: "hits" }, to: { child: "fx" } },
      // @ts-expect-error: a sound moves no live control.
      { from: { child: "lfo" }, to: { child: "drop", live: "rate" } },
      // @ts-expect-error: and fires no trigger.
      { from: { child: "lfo" }, to: { child: "drop", trigger: "hit" } },
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
      // @ts-expect-error: a live control is an input of a child, no output of the parent.
      { from: { child: "lfo", number: "wave" }, to: { live: "rate" } },
    ],
  }),
});

tool({
  name: "conductor",
  title: "Conductor",
  when: "Never",
  doc: "Plays its children.",
  state: {},
  controls: { hit: trigger() },
  behaviour: (): Declaration => ({ connections: [] }),
  tick: ({ set, fire, play, send }) => {
    fire("hit");
    set({ child: "lfo", live: "rate" }, 2);
    fire({ child: "drums", trigger: "kick" }, { at: 1 });
    play(60, { child: "synth", hold: true });
    send({ type: "noteOff", pitch: 60 }, { child: "synth" });
    send({ type: "cc", controller: 64, value: 1 }, { child: "synth", at: 1 });
    send({ type: "bend", value: -0.5 });
    // @ts-expect-error: a misspelled event.
    send({ type: "noteoff", pitch: 60 });
    // @ts-expect-error: a note on needs its velocity.
    send({ type: "noteOn", pitch: 60 });
    // @ts-expect-error: a key is let go of with a `noteOff`.
    send({ type: "noteOff", pitch: 60, velocity: 0 });
    // @ts-expect-error: a trigger of a child is fired, not set.
    set({ child: "drums", trigger: "kick" }, 1);
    // @ts-expect-error: and a live control of a child set, not fired.
    fire({ child: "lfo", live: "rate" });
    // @ts-expect-error: a knob of a child is the composition: a cable moves it.
    set({ child: "filter", number: "cutoff_hz" }, 800);
  },
});

tool({
  name: "recorder",
  title: "Recorder",
  when: "Never",
  doc: "Keeps what it hears and plays it back.",
  kind: "instrument",
  state: {},
  memory: () => ({ heard: [] as HeardEvent[] }),
  onNotes: ({ memory, send }, event) => {
    memory.heard.push(event);
    send(event, { at: event.at + 1 });
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
  doc: "Saves each take as a child, and edits its sequencer.",
  state: { count: number({ min: 0, max: 100 }) },
  behaviour: (): Declaration => ({ connections: [] }),
  card: ({ time, transport, children }) => h("div", {}, `${children.length} at ${time} s, ${transport.bpm} bpm`),
  onNotes: async ({ update, children, state, transport }, event) => {
    const beats: number = (event.at * transport.bpm) / 60;
    const taken: number = children.length;
    update("Save a take", ({ state, children }) => {
      state.count = taken + 1;
      children.push({ name: `take-${taken}`, tool: "take", state: { notes: [] } });
      const bass = children.find((child) => child.name === "bass")!.state as SequencerState;
      bass.patterns?.push({ length: 3840, notes: [{ start: 0, length: 480, pitch: event.type === "noteOn" ? event.pitch : 60, velocity: 100 }] });
      // @ts-expect-error: a child's state is any tool's: say which it is.
      children[0]!.state.patterns;
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

tool({
  name: "room",
  title: "Room",
  when: "Never",
  doc: "Two reverbs, one with its numbers as knobs.",
  state: { ...devices.reverb.fields, ...prefixed("far_", devices.reverb.fields), drive: knob({ min: 0, max: 1, default: 0 }) },
  sound: (fields) => {
    const near = devices.reverb(input, fields);
    const far = devices.reverb({ left: near.left, right: near.right }, { mix: fields.far_mix, freeze: true });
    // @ts-expect-error: the reverb has no number `size_m`.
    devices.reverb(input, { size_m: 1 });
    // @ts-expect-error: a toggle of the record is written as it is, not played as a signal.
    devices.reverb(input, { freeze: fields.drive });
    // @ts-expect-error: the fields of the second reverb are named with their prefix.
    fields.far_freeze;
    devices.filter(far, { type: "high_pass", cutoff_hz: fields.drive });
    // @ts-expect-error: a choice of a device is one of its options.
    devices.filter(far, { type: "hi_pass" });
    // A band takes the defaults of its place for what it leaves out.
    devices.eq(far, { bands: [{ gain_db: 3 }] });
    return far;
  },
});

const parts = { bass: "sequencer", fx: "filter", drop: "rain" } as const;

tool({
  name: "loops",
  title: "Loops",
  when: "Never",
  doc: "Names its children, so their ports and records are typed.",
  state: {},
  children: parts,
  behaviour: ({ children }): Declaration<typeof parts> => {
    const bass = children.find((child) => child.name === "bass");
    const length: number | undefined = bass?.state.patterns?.[0]?.length;
    if (length === 0) return { connections: [] };
    return {
      connections: [
        { from: { child: "drop", number: "level" }, to: { child: "fx", number: "cutoff_hz" } },
        { from: { child: "drop", number: "level" }, to: { child: "bass", live: "launch" } },
        { from: { child: "bass", notes: "notes" }, to: { child: "drop", notes: "notes" } },
        { from: { child: "fx" }, to: { port: "audio" } },
        // @ts-expect-error: the filter has no number `cutoff`.
        { from: { child: "drop", number: "level" }, to: { child: "fx", number: "cutoff" } },
        // @ts-expect-error: and no live control.
        { from: { child: "drop", number: "level" }, to: { child: "fx", live: "launch" } },
        // @ts-expect-error: a child it does not name.
        { from: { child: "synth" }, to: { port: "audio" } },
      ],
    };
  },
  tick: ({ set, play, update }) => {
    set({ child: "bass", live: "launch" }, 1);
    // @ts-expect-error: the sequencer's live control is `launch`.
    set({ child: "bass", live: "lunch" }, 1);
    // @ts-expect-error: a child it does not name.
    play(60, { child: "synth" });
    update("Edit", ({ children }) => {
      for (const child of children) {
        if (child.name === "fx") child.state.type = "high_pass";
        // @ts-expect-error: a choice of a child is one of its options.
        if (child.name === "fx") child.state.type = "notches";
        // @ts-expect-error: a field the record of a sequencer does not have.
        if (child.name === "bass") child.state.pattern = [];
      }
      children.push({ name: "fx", tool: "filter", state: { cutoff_hz: 500 } });
      // @ts-expect-error: the child `fx` is a filter.
      children.push({ name: "fx", tool: "delay", state: {} });
    });
  },
});

const loose = { bass: "sequencer" };

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
  children: { bass: "sequencer" },
  behaviour: (): Declaration => ({ connections: [] }),
});

const drone = tool({
  name: "drone",
  title: "Drone",
  when: "Never",
  doc: "A tool of the project that a parent names by what `tool` returns.",
  kind: "source",
  state: { pitch: knob({ min: 0, max: 127, default: 60 }), name: text() },
  controls: { swell: live({ min: 0, max: 1, default: 0 }), hit: trigger() },
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
        { from: { child: "lead", bang: "onset" }, to: { child: "lead", trigger: "hit" } },
        { from: { child: "lead", number: String(pitch) }, to: { child: "lead", live: "swell" } },
        { from: { child: "lead" }, to: { child: "fx" } },
        // @ts-expect-error: the drone has no knob `pitc`.
        { from: { child: "lead", number: "level" }, to: { child: "lead", number: "pitc" } },
        // @ts-expect-error: its input of notes is `notes`.
        { from: { child: "lead", notes: "melody" }, to: { child: "lead", notes: "keys" } },
      ],
    };
  },
  tick: ({ set, fire, update }) => {
    set({ child: "lead", live: "swell" }, 1);
    fire({ child: "lead", trigger: "hit" });
    // @ts-expect-error: the drone has no live control `swel`.
    set({ child: "lead", live: "swel" }, 1);
    // @ts-expect-error: and no trigger `hits`.
    fire({ child: "lead", trigger: "hits" });
    update("Edit", ({ children }) => {
      for (const child of children) {
        if (child.name === "lead") child.state.pitch = 62;
        // @ts-expect-error: a field the drone does not have.
        if (child.name === "lead") child.state.pitches = 62;
      }
    });
  },
});
