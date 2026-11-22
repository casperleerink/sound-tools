// What all code of a tool reads of the project: the clock, the keys, and what a menu and the
// window's offers are made of.

import type { Frozen } from "./tools";

/**
 * The clock of the project, one for all tools: where the piece plays and how fast. A binding
 * that reads `playing`, `repeat`, `click`, `computerKeys`, `tempo_map` or `sections` runs
 * again when it changes. `tick`, `bar`, `beat` and `bpm` move on every frame: read them in a
 * handler, `tick` or `onKey`, and show them with `now.tick` and `now.bpm`.
 */
export interface Clock {
  readonly playing: boolean;
  /** The position in ticks, 960 to a quarter note, as the code runs. */
  readonly tick: number;
  /** The bar and the beat of `tick`, from 1. */
  readonly bar: number;
  readonly beat: number;
  /** The tempo at `tick`, in beats per minute. */
  readonly bpm: number;
  /** The ticks playback goes back from the end to the start of, while it repeats. */
  readonly repeat: { readonly from: number; readonly to: number } | null;
  /** Whether the click sounds the beats. Not saved. */
  readonly click: boolean;
  /** Whether the computer keys play notes, as a MIDI keyboard does. A switch of this machine, not saved; cmd-K turns it too. */
  readonly computerKeys: boolean;
  /** Whether a drag of a knob or a fader that a lane moves writes into the lane while the piece plays, from where it is grabbed to where it is let go. Not saved. */
  readonly writeAutomation: boolean;
  /** As `project.json` holds them: the time signatures and the tempo changes. */
  readonly tempo_map: Frozen<TempoMap>;
  /** As `project.json` holds them: the named parts of the piece, sorted by bar. */
  readonly sections: Frozen<Section[]>;
  play(): void;
  pause(): void;
  /** Pauses and goes back to the start. */
  stop(): void;
  /** Playback goes on from `to`: at once, or with `atTick` once it reaches that tick, on its frame. */
  jump(to: number, options?: { atTick?: number }): void;
  /** Playback goes back from `to` to `from` while it is set; `null` plays on. Not saved. */
  setRepeat(range: { from: number; to: number } | null): void;
  setClick(on: boolean): void;
  setComputerKeys(on: boolean): void;
  setWriteAutomation(on: boolean): void;
  /**
   * Changes the tempo map and the sections: `change` gets a copy to edit, and what `project.json`
   * does not accept is refused whole. Updates of one `label` in a row are one undo step.
   */
  update(label: string, change: (song: { tempo_map: TempoMap; sections: Section[] }) => void): void;
}

/**
 * The time signatures, as runs of bars; the tempo from a tick on, moving evenly to `to` by the
 * next change when it has one (a rit. or an accel.); and the fermatas, each holding the beat
 * from its tick `hold` times as long (1 to 8).
 */
export interface TempoMap {
  time_signatures: { signature: string; bars: number }[];
  tempo_changes: { tick: number; bpm: number; to?: number }[];
  fermatas?: { tick: number; hold: number }[];
}

/** A tempo map as the clock gives it, which code reads and does not change. */
type ReadTempoMap = {
  readonly time_signatures: readonly { readonly signature: string; readonly bars: number }[];
  readonly tempo_changes: readonly { readonly tick: number; readonly bpm: number; readonly to?: number }[];
  readonly fermatas?: readonly { readonly tick: number; readonly hold: number }[];
};

/** A 32nd note, how finely the clock cuts a tempo that moves. */
const GLIDE_STEP = 120;
const [SLOWEST, FASTEST] = [10, 1000];
const tempo = (bpm: number) => Math.round(Math.min(Math.max(bpm, SLOWEST), FASTEST) * 1000) / 1000;

/** The ticks of a beat of the time signature that plays at `tick`. */
function beatAt(runs: ReadTempoMap["time_signatures"], tick: number): number {
  let start = 0;
  for (const [index, run] of runs.entries()) {
    const [count, unit] = run.signature.split("/").map(Number);
    const bar = ((count ?? 4) * 3840) / (unit ?? 4);
    if (index === runs.length - 1 || tick < start + run.bars * bar) return 3840 / (unit ?? 4);
    start += run.bars * bar;
  }
  return 960;
}

/**
 * The tempo as it plays, in plain steps, as the clock plays it: a change that moves cut into a
 * step every 32nd note, each at the tempo halfway through it; a fermata's beat at the tempo
 * divided by its hold.
 */
export function tempoSteps(map: ReadTempoMap): { tick: number; bpm: number }[] {
  const changes = map.tempo_changes;
  const steps = changes.flatMap(({ tick, bpm, to }, index) => {
    const end = changes[index + 1]?.tick;
    if (to === undefined || end === undefined) return [{ tick, bpm }];
    const length = end - tick;
    return Array.from({ length: Math.ceil(length / GLIDE_STEP) }, (_, step) => {
      const at = tick + step * GLIDE_STEP;
      return { tick: at, bpm: tempo(bpm + ((to - bpm) * (Math.min(at + GLIDE_STEP / 2, end) - tick)) / length) };
    });
  });
  const split = (tick: number) => {
    const index = steps.findIndex((step) => step.tick >= tick);
    if (steps[index]?.tick === tick) return;
    const at = index === -1 ? steps.length : index;
    if (at > 0) steps.splice(at, 0, { tick, bpm: steps[at - 1]!.bpm });
  };
  for (const { tick, hold } of map.fermatas ?? []) {
    const end = tick + beatAt(map.time_signatures, tick);
    split(tick);
    split(end);
    // Slower than a composer types, as the clock holds it.
    for (const step of steps) if (step.tick >= tick && step.tick < end) step.bpm = Math.max(Math.round((step.bpm * 1000) / hold), 1) / 1000;
  }
  return steps;
}

/** A part of the piece from the start of `bar`, counted from 1, up to the next section. */
export interface Section {
  bar: number;
  name: string;
}

/** The keys that type a character other than a letter. */
export const SYMBOL_KEYS = [
  "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "-", "=", "[", "]", ";", "'", ",", ".", "/", "`", "\\"
] as const;
/** What the symbol keys type with shift. */
export const SHIFTED_KEYS = [
  "!", "@", "#", "$", "%", "^", "&", "*", "(", ")", "_", "+", "{", "}", ":", '"', "<", ">", "?", "~", "|"
] as const;
/** The keys of the computer keyboard, as the window names them. */
export const PLAIN_KEYS = [
  "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
  "t", "u", "v", "w", "x", "y", "z", ...SYMBOL_KEYS, "space", "enter", "escape", "backspace", "delete",
  "tab", "up", "down", "left", "right", "home", "end", "pageup", "pagedown", "f1", "f2", "f3",
  "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12"
] as const;
/** A key of the computer keyboard by its name. */
export type PlainKey = (typeof PLAIN_KEYS)[number];
type SymbolKey = (typeof SYMBOL_KEYS)[number];
type Held = `${"" | "cmd-"}${"" | "ctrl-"}${"" | "alt-"}`;
/** A key by name, with cmd, ctrl and alt held first and in that order: `"space"`, `"cmd-c"`, `"cmd-alt-left"`. */
export type KeyName = `${Held}${PlainKey}`;
type ActionHeld = `${"" | "cmd-"}${"" | "alt-"}`;
/**
 * A key an action has, with cmd, alt and shift held first and in that order: `"space"`,
 * `"shift-up"`, `"cmd-shift-z"`. `cmd` is ctrl on Windows and Linux, so there is no `ctrl-`: a
 * key named here works on every system. A symbol key with shift is named by what it then
 * types, with no shift: `"?"`, `"cmd-+"`. The app's own keys are none of an action's.
 */
export type ActionKey = Exclude<`${ActionHeld}${"" | "shift-"}${Exclude<PlainKey, SymbolKey>}` | `${ActionHeld}${SymbolKey | (typeof SHIFTED_KEYS)[number]}`, AppKey>;
/** An action by its tool and its name: `"song.record"`. The app's own are `"app.undo"`, `"app.redo"` and `"app.computer_keys"`. */
export type ActionId = `${string}.${string}`;
/** What a `shortcut` draws: a key, or an action, whose first key it then draws as the composer has it now, and nothing while it has none. */
export type Shortcut = KeyName | ActionId;
/** The keys the app keeps: undo, redo, new, open, quit, settings, the panel, the computer keys, and tab. */
export type AppKey = `${"cmd" | "ctrl"}-${"z" | "n" | "o" | "q" | "l" | "b" | "k"}` | "cmd-shift-z" | "cmd-alt-z" | "cmd-," | "tab" | "shift-tab";

/** An item of `openMenu`. */
export interface MenuItem {
  label: string;
  disabled?: boolean;
  /** A tick beside it. */
  checked?: boolean;
  icon?: IconName;
  /** A quiet second line, such as who made a plugin. */
  detail?: string;
  /** The heading it is under, with the items next to it of the same group. */
  group?: string;
  /** A line above it, between it and the items before. */
  separator?: boolean;
  /** The keys that do it too, drawn at the end of its row. */
  shortcut?: Shortcut;
}

/** A device `offers` gives. */
export interface Offer {
  /** What `addOffer` names it by. */
  key: string;
  name: string;
  detail: string | null;
  /** The group a picker lists it under, such as "Built-in" or "Plugins". */
  group: string;
  /** The icon a picker shows beside it. */
  icon: IconName;
  /** Why it cannot be added here, or `null` when it can. */
  unavailable: string | null;
}

/** What `offers` gives: the devices, and what the window's pickers say under them, such as whose trademark a plugin format is. */
export interface Offers {
  offers: Offer[];
  notes: string[];
  /** The key of the offer at the child `offers` was asked `at`, which a picker ticks. */
  here: string | null;
}

/** How a menu of `openMenu` shows. */
export interface MenuOptions {
  /** Quiet lines under the items. */
  notes?: readonly string[];
  /** In points; 320 when left out. */
  width?: number;
  /** Past this many points the items scroll; 420 when left out. */
  maxHeight?: number;
  /** A field above the items that keeps those whose label holds what is typed. */
  search?: boolean;
}

/** A number `numbers` gives: what a lane or a cable moves, from `min` to `max`, as its knob travels them. */
export interface MovedNumber {
  name: string;
  /** What a person reads for it: as its device names it, such as a plugin's parameter, or its name in plain words, `Osc 1 position`. */
  label: string;
  min: number;
  max: number;
  /** `"log"` for a frequency or a time, `"fader"` for a volume in dB (`-inf` at the bottom). */
  scale: "linear" | "log" | "fader";
  /** How a value of it reads: by the unit its name ends in, `pan` for a pan, `percent` for a share of 0 to 1; none for a plugin's. */
  unit?: "hz" | "db" | "ms" | "seconds" | "percent" | "pan";
  /** What its record holds now; `null` for a number that has none, such as one of a plugin that is not loaded. */
  value: number | null;
}

/** Which modifier keys were held. */
export interface Modifiers {
  shift: boolean;
  alt: boolean;
  cmd: boolean;
  ctrl: boolean;
}

export type IconName =
  | "arrow-left" | "arrow-right" | "arrow-up" | "audio-lines" | "automation" | "bot" | "check"
  | "chevron-down" | "chevron-left" | "chevron-right" | "chevron-up" | "circle" | "circle-alert"
  | "circle-check" | "claude" | "copy" | "device-analyzer" | "device-compressor" | "device-delay"
  | "device-drum-pad" | "device-eq" | "device-filter" | "device-gate" | "device-limiter"
  | "device-modulation" | "device-plugin" | "device-reverb" | "device-sampler" | "device-saturator"
  | "device-synth" | "device-utility" | "device-wavetable" | "ellipsis" | "eq-bell" | "eq-high-cut"
  | "eq-high-shelf" | "eq-low-cut" | "eq-low-shelf" | "eq-notch" | "eye" | "eye-off" | "folder" | "info"
  | "keyboard-music" | "loader-circle" | "lock" | "lock-open" | "metronome" | "mic" | "minus" | "music"
  | "panel-left" | "pause" | "pencil" | "piano" | "play" | "plus" | "power" | "redo-2" | "search" | "section"
  | "send" | "settings" | "shield" | "skip-back" | "skip-forward" | "sliders-horizontal" | "sparkles"
  | "square" | "tempo-change" | "time-signature" | "trash-2" | "triangle-alert" | "undo-2" | "volume-2"
  | "volume-x" | "x" | "zap";
