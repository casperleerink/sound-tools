// Types an agent's tool must get right. A test of the crate checks this file with `tsc` next to
// the SDK: every `@ts-expect-error` line must be an error, and nothing else.
import { input, list, number, output, ownSound, rise, text, tool, trigger, type Declaration } from "./sdk";

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
    const connections = [{ from: { child: name + steps + plain }, to: ownSound }];
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
  name: "conductor",
  title: "Conductor",
  when: "Never",
  doc: "Plays its children.",
  state: {},
  controls: { hit: trigger() },
  behaviour: (): Declaration => ({ connections: [] }),
  tick: ({ set, fire, play, release }) => {
    fire("hit");
    set({ child: "lfo", live: "rate" }, 2);
    fire({ child: "drums", trigger: "kick" }, { at: 1 });
    play(60, { child: "synth", hold: true });
    release(60, { child: "synth" });
    // @ts-expect-error: a trigger of a child is fired, not set.
    set({ child: "drums", trigger: "kick" }, 1);
    // @ts-expect-error: and a live control of a child set, not fired.
    fire({ child: "lfo", live: "rate" });
    // @ts-expect-error: a knob of a child is the composition: a cable moves it.
    set({ child: "filter", number: "cutoff_hz" }, 800);
  },
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
