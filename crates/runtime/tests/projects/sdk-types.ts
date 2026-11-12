// Types an agent's tool must get right. A test checks this file with `bun check` next to the
// SDK of the built-in tools: every `@ts-expect-error` line must be an error, and nothing else.
import { abs, alpha, audio, blend, Controls, Display, Fader, h, highpass, input, Knob, Label, Live, lowpass, Notes, now, number, list, notes, NumberBox, output, Page, Path, Readout, rise, Row, SegmentedControl, Select, Slider, Steps, text, Text, Toggle, tool, Transport, bang, Wave, type Color, type Declaration, type HeardEvent, type Look, type Style, type TimelineState } from "./sdk";

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

// Sound ports beyond `audio`: a parent's connections name them, checked against each tool.
const duck = tool({
  name: "duck",
  title: "Duck",
  when: "Never",
  doc: "Turns down what comes in while its sidechain plays.",
  state: {},
  inputs: { sidechain: audio() },
  sound: ({ sidechain }) => input.times(abs(sidechain.left).lt(0.1)),
});
const split = tool({
  name: "split",
  title: "Split",
  when: "Never",
  doc: "Its lows out of `low`.",
  state: {},
  outputs: { low: audio() },
  sound: () => ({ audio: highpass(input, 200), low: lowpass(input, 200) }),
});
tool({
  name: "unsplit",
  title: "Unsplit",
  when: "Never",
  doc: "A sound that gives no `low`.",
  state: {},
  outputs: { low: audio() },
  // @ts-expect-error: it gives each of its outputs.
  sound: () => ({ audio: input }),
});
tool({
  name: "mixed",
  title: "Mixed",
  when: "Never",
  doc: "A sound whose outputs are one stereo and one not.",
  state: {},
  outputs: { low: audio() },
  // @ts-expect-error: its outputs are each one signal or each { left, right }.
  sound: () => ({ audio: { left: input, right: input }, low: input }),
});
const ported = { duck, split } as const;

tool({
  name: "ported",
  title: "Ported",
  when: "Never",
  doc: "Keys its duck with the lows of its split.",
  state: {},
  children: ported,
  behaviour: (): Declaration<typeof ported> => ({
    connections: [
      { from: { child: "split", port: "low" }, to: { child: "duck", port: "sidechain" } },
      { from: { child: "duck" }, to: { port: "audio" } },
      // @ts-expect-error: the duck has no input `sidechian`.
      { from: { child: "split" }, to: { child: "duck", port: "sidechian" } },
      // @ts-expect-error: and `low` is an output of the split, not an input.
      { from: { child: "duck" }, to: { child: "split", port: "low" } },
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
    // @ts-expect-error: no key is called `spacebar`.
    if (key.key === "spacebar") clock.play();
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

tool({
  name: "acts",
  title: "Acts",
  when: "Never",
  doc: "Actions.",
  state: { count: number({ min: 0, max: 10 }) },
  sound: () => 0,
  actions: {
    count: { title: "Count", keys: ["c", "shift-up", "cmd-shift-x", "?", "cmd-+"], everywhere: true, run: ({ update }) => update("Count", ({ state }) => { state.count += 1; }) },
    ask: { title: "Ask", run: async ({ confirm }) => { await confirm({ message: "Sure?", confirm: "Yes" }); } },
    pass: { title: "Pass", keys: ["c"], run: ({ state }) => state.count > 0 },
    // @ts-expect-error: shift is held last, after cmd, ctrl and alt.
    wrong: { title: "Wrong", keys: ["shift-cmd-z"], run: () => {} },
    // @ts-expect-error: cmd is ctrl on Windows and Linux, so an action names no ctrl.
    control: { title: "Control", keys: ["ctrl-r"], run: () => {} },
    // @ts-expect-error: redo is the app's.
    taken: { title: "Taken", keys: ["cmd-shift-z"], run: () => {} },
    // @ts-expect-error: a symbol key with shift is named by what it types, "!".
    shifted: { title: "Shifted", keys: ["shift-1"], run: () => {} },
  },
});

// A shortcut names a key, or an action by its tool and its name.
Toggle({ label: "C", tooltip: "Count", shortcut: "acts.count", on: true });
Toggle({ label: "C", tooltip: "Count", shortcut: "cmd-c", on: true });
// @ts-expect-error: no key is called `del`.
Toggle({ label: "C", tooltip: "Count", shortcut: "del", on: true });

// A select's value is typed by its options.
const lanes = [{ value: "velocity", label: "Velocity" }, { value: "pedal", label: "Pedal" }] as const;
const lane = (): "velocity" | "pedal" => "velocity";
Select({ value: lane, options: lanes, onChange: (picked: "velocity" | "pedal") => picked });
// @ts-expect-error: no option is "bend".
Select({ value: lane, options: lanes, onChange: (picked: "bend") => picked });

// A toggle shows one face: a label, an icon with its count, or a dot.
Toggle({ icon: "automation", count: 2, tooltip: "Automation", on: true });
// @ts-expect-error: an icon toggle shows no label.
Toggle({ icon: "automation", label: "Automation", on: true });

// A look takes only what a look changes, even from a `Style`.
const look: Look = { background: "fill-strong" };
const sized = { width: 120, background: "red" } satisfies Style;
// @ts-expect-error: a width is no part of a look.
const sizedLook: Look = sized;
h("div", { style: { hover: look, pressed: sizedLook } });

// A view names a role, a hue or a step of a scale: a step of grey or a size no scale has is refused.
h("div", { style: { background: "fill", color: "text-muted", gap: "sm", padding: 3, radius: "full" } });
// @ts-expect-error: a step of the grey scale is no role.
h("div", { style: { color: "gray-700" } });
// @ts-expect-error: white at an opacity is no role.
h("div", { style: { background: "alpha/10" } });
// @ts-expect-error: no gap is `huge`.
h("div", { style: { gap: "huge" } });
// @ts-expect-error: a control is `sm`, `md` or `lg`.
Toggle({ label: "C", on: true, size: "xs" });
// @ts-expect-error: a text is a step of the type scale.
Text({ variant: "small", children: "No such step" });

// A page lays out its content, or is one surface to the window's edges: it takes no width.
Page({ flush: true });
// @ts-expect-error: a part is as wide as what is in it, or grows.
Page({ maxWidth: 720 });

// A card's view gives its parts with no row around them. A wave needs no colour, and a label
// takes a step of the type scale.
tool({
  name: "parts",
  title: "Parts",
  when: "A test",
  doc: "A card of a display and a knob.",
  state: { level: number({ min: 0, max: 1 }) },
  view: ({ now }) => [
    Display({ caption: "Level", children: [Wave({ list: now.history(now.value("level"), 120) }), Label({ x: 0, y: 0, text: "Level", variant: "heading", tone: "muted" })] }),
    Controls({ children: Knob({ field: "level", min: 0, max: 1, default: 0 }) }),
  ],
});
// @ts-expect-error: no step of the type scale is `small`.
Label({ x: 0, y: 0, text: "C2", variant: "small" });
// @ts-expect-error: a label has no size in points.
Label({ x: 0, y: 0, text: "C2", size: 13 });

// A control names a `field`, and takes its range, its label and its options from it: nothing else is needed.
Knob({ field: "cutoff" });
Slider({ field: "cutoff" });
Fader({ field: "mix.gain_db", length: 100 });
NumberBox({ field: "bpm", key: "bpm", width: 40 });
SegmentedControl({ field: "mode" });
Select({ field: "mode" });
Steps({ field: "kick" });
// A switch on a boolean field says the field's label and turns it; else it says its face and what it shows.
Toggle({ field: "hold" });
Toggle({ field: "hold", icon: "lock" });
// @ts-expect-error: a field or `on`, not both.
Toggle({ field: "hold", on: true });
// @ts-expect-error: a switch on what code keeps has a face.
Toggle({ on: true });
// A control on a field is a cell of `Controls` by itself.
Controls({ children: [Knob({ field: "cutoff" }), SegmentedControl({ field: "mode" }), Select({ field: "mode", size: "sm" }), Toggle({ field: "hold" })] });
// The ring of the step that plays shows while the piece plays, unless the steps run on their own.
Steps({ field: "kick", playing: "step" });
Steps({ field: "kick", playing: "step", ownClock: true });
// A row that changes the row over it is secondary: quieter, and without the ring.
Steps({ field: "accents", playing: "step", secondary: true });
// Each can still be said, and a number inside a child's record is a field too.
Knob({ field: "cutoff", min: 0, max: 1, default: 0 });
Knob({ field: "kit.pads.36.volume_db", label: "Level" });
// @ts-expect-error: a field always shows what plays, so no control says it.
Knob({ field: "cutoff", played: true });
// @ts-expect-error: nor a live value.
now.value("cutoff", { played: true });
// Or it shows what code keeps and hears it with `onChange`, and then says its range or its options.
Knob({ value: () => 0.5, onChange: (value: number) => value, min: 0, max: 1, default: 0 });
// @ts-expect-error: what code keeps has no field to tell its range.
Knob({ value: () => 0.5, onChange: (value: number) => value });
// @ts-expect-error: nor its options.
SegmentedControl({ value: "a", onChange: (picked: string) => picked });
// One prop says how much text a knob shows.
Knob({ field: "cutoff", text: "value" });
// @ts-expect-error: a knob has no size.
Knob({ field: "cutoff", size: "sm" });
// A number stays quiet until its control is touched: a knob's own, or the one beside a fader.
Knob({ field: "cutoff", quiet: true });
Readout({ field: "cutoff", width: 44, quiet: () => true });
// @ts-expect-error: a fader says no number, so it has none to keep quiet.
Fader({ field: "cutoff", length: 100, quiet: true });
// @ts-expect-error: a value code keeps is no field name, and needs `onChange`.
Select({ value: "velocity", options: lanes });
// @ts-expect-error: a field or what code keeps, not both.
Select({ field: "mode", value: lane, onChange: (picked: "velocity" | "pedal") => picked, options: lanes });
// @ts-expect-error: `show` is for what code keeps; a field shows itself.
NumberBox({ field: "bpm", show: 120, key: "bpm", width: 40 });

// A path starts at a point, not with a curve.
Path({ points: [[0, 0], [5, 0, 10, 10]], color: "#ffffff" });
// @ts-expect-error: its first point is `[x, y]`.
Path({ points: [[5, 0, 10, 10]], color: "#ffffff" });

// Drawing by hand needs no cast: a number as a live value, points made in a loop, a colour that is a variable.
const octaves: Live = Live.of(2).pow(now.value("pitch").over(12));
const points: (readonly [Live, Live])[] = [];
for (let place = 0; place <= 8; place++) points.push([octaves.times(place), octaves]);
const marks = ["teal", "sky"] as const;
for (const mark of marks) Path({ points, color: alpha(mark, 40), width: 2 });
// @ts-expect-error: a colour is one of the theme.
alpha("grey", 40);
// Colours kept in a list, typed by what the list holds or left as they are written.
const voices: { title: string; color: Color }[] = [{ title: "Root", color: "blue" }, { title: "Reed", color: "teal/60" }];
for (const voice of voices) Path({ points, color: blend(alpha(voice.color, 25), voice.color, now.value("level")), width: 2 });
const drones = [{ title: "Choir", color: "mauve" }, { title: "Glass", color: "peach" }] as const;
for (const drone of drones) Wave({ list: now.scope(), color: drone.color });
// @ts-expect-error: a colour of a list that says nothing is any text.
for (const drone of [{ color: "mauve" }]) Wave({ list: now.scope(), color: drone.color });

// A number from 0 to 1 reads as a percent with no unit said; the plain one is the one that says so.
number({ min: 0, max: 1, default: 0.5 });
number({ min: 0, max: 1, default: 0.5, unit: "plain" });
const whole: { integer: true } = number({ min: 0, max: 1, integer: true, unit: "plain" });
// @ts-expect-error: plain is said once, on the field, and no control says it again.
Knob({ field: "seed", unit: "plain" });
// @ts-expect-error: and it has one word.
number({ min: 0, max: 1, unit: "none" });

// A field says its unit, in music's own: a knob on it needs none.
number({ min: -12, max: 12, unit: "semitones" });
number({ min: 24, max: 84, unit: "pitch" });
number({ min: -1, max: 1, unit: "pan" });
Label({ x: 0, y: 0, text: now.value("detune"), unit: "cents" });
// @ts-expect-error: beats are no unit of a number.
number({ min: 0, max: 4, unit: "beats" });

// A scope is of what the tool plays, or of a child by its path; a wave and notes fill their space unless they say a place.
Wave({ list: now.scope() });
Wave({ list: now.scope({ frames: 1024, channel: 0 }) });
Wave({ list: now.scope("bass/synth", { frames: 1024 }), y: 0.5, height: 0.5 });
Notes({ list: now.take("rec"), color: "text" });
// @ts-expect-error: a scope is of sound, not of a number.
Wave({ list: now.scope(3) });

// A display says its scale: lines at places across, with a word where given, lines at places down and a zero line.
Display({ scale: { x: [0.25, [0.5, "1k"]], y: [0.25, 0.75] }, zero: 0.5 });
Display({ scale: () => ({ y: [0.5] }), zero: () => 0.5 });
// @ts-expect-error: a mark is its place, then its word.
Display({ scale: { x: [["1k", 0.5]] } });
// @ts-expect-error: a line across has no word.
Display({ scale: { y: [[0.5, "0 dB"]] } });
// @ts-expect-error: one zero line.
Display({ zero: [0.5] });

// A row of parts takes a gap of the scale and nothing of a style.
Row({ gap: "lg", grow: true, children: [Display({}), Controls({ children: [Knob({ field: "cutoff" }), Knob({ field: "level" })] })] });
// @ts-expect-error: a gap of a row is a step of the scale.
Row({ gap: 10 });
// @ts-expect-error: a row is no div to style.
Row({ style: { padding: "lg" } });

// A transport shows the bars by what its tool plays, or as it is told.
Page({ header: Transport({ time: true }) });
// @ts-expect-error: the parts of a transport are not picked one by one.
Transport({ metronome: false });
