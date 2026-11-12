// The parts of a view that fills the window: the page, the panels on it and the transport.

import { Meter, NumberBox } from "./components";
import { Button, h, Label, Space, Style, Text, Toggle, Tone } from "./drawing";
import { host } from "./host";
import { Clock, Live, now, TempoMap } from "./live";
import { Child, currentCard, element, homeInControls, UiNode } from "./nodes";
import { Accessor, Bind, untrack } from "./reactivity";
import { Action, Frozen } from "./tools";

export interface PageProps {
  /**
   * What it shows in the window's title bar, in the middle of the room right of the project menu:
   * a `Transport` when left out, so every tool plays and stops the same way; its own controls
   * beside one, `<><Transport /><Button ... /></>`; or `false` for a tool that always sounds. A
   * page that is not the window's shows none.
   */
  header?: Child | false;
  /** Its content reaches the edges of the window and does not scroll: one surface, such as a canvas or a timeline. */
  flush?: boolean;
  children?: Child;
}

/**
 * The top of a view that fills the window: its content one under the other with the room of the
 * app around and between, scrolling down where the window is too small for it. A part is as wide
 * as what is in it and sits in the middle of the window, so a page smaller than the window is in
 * its middle. A part with `grow` is as wide as the window and takes the room that is left, down
 * to what it needs: a `Space` its `height`.
 */
export function Page({ header, children, flush }: PageProps): UiNode {
  const shown = header === undefined ? Transport() : header;
  const bar = shown === false ? null : element("div", { titleBar: true, style: { direction: "row", align: "center", gap: "md" } }, [shown]);
  if (flush) return h("div", { style: { grow: true } }, bar, children);
  const content = element("div", { layout: "page", style: { grow: true, gap: "lg", justify: "center" } }, [children]);
  return h("div", { style: { grow: true } }, bar, h("div", { style: { grow: true, scroll: "y", padding: "xl" } }, content));
}

export interface PanelProps {
  /** Names it for tests, `div-<key>`. */
  key?: string;
  title?: Bind<string>;
  /** Takes the room left in the page, or in the div it is in, with the other parts that grow: for a surface that needs the room, a canvas or a piano roll, not for a picture of a sound. As wide as its content when left out. */
  grow?: boolean;
  /** In points, for one whose content changes beside a panel that grows, such as a list. */
  width?: number;
  children?: Child;
}

/**
 * A surface on a page, as a card looks, its content one under the other. The parts of a card go
 * in it side by side: `<Panel title="Drone"><Row><Display>...</Display><Controls>...</Controls></Row></Panel>`.
 */
export function Panel({ key, title, grow, width, children }: PanelProps): UiNode {
  const panel = h("div", { key, style: { grow, width, keep: width !== undefined, gap: "md", padding: "lg", radius: "lg", background: "surface", borderColor: "border", borderWidth: 1 } },
    title === undefined ? null : Text({ variant: "heading", children: title }), children);
  homeInControls(panel);
  return panel;
}

/** What a `Transport` gives its tool as the action `play`, which its button shows the key of. */
const playPause: Action<{ clock: Clock }> = {
  title: "Play or pause",
  keys: ["space"],
  everywhere: true,
  run: ({ clock }) => (clock.playing ? clock.pause() : clock.play()),
};

const BEAT = 960;
/** The note values a time signature takes. */
const NOTES = [1, 2, 4, 8, 16, 32] as const;

/** The run of `runs` that plays at `tick`, the last going on, with its beats and note value. */
function signatureAt(runs: Frozen<TempoMap>["time_signatures"], tick: number): { run: number; beats: number; note: number } {
  let start = 0;
  for (const [run, { signature, bars }] of runs.entries()) {
    const [beats = 4, note = 4] = signature.split("/").map(Number);
    start += (bars * beats * 4 * BEAT) / note;
    if (tick < start || run === runs.length - 1) return { run, beats, note };
  }
  return { run: 0, beats: 4, note: 4 };
}

/** A readout of a tick of the clock as it moves, in the pill's text. */
const clockText = (key: string, width: number, unit: "position" | "time", tone: Tone, text: Live = now.tick) =>
  Space({ key, width, height: 18, children: Label({ x: 0, y: 0, text, unit, variant: "body", tone }) });

/** A word of the pill `x` points into its room, such as the unit of the number before it. */
const word = (key: string, width: number, x: number, text: string) =>
  Space({ key, width, height: 18, children: Label({ x, y: 2, text, tone: "muted" }) });

/** The built-in children that play in the bars of the piece. */
const IN_TIME = ["timeline", "lane", "region", "recorder"];

/** Whether `tool` plays in the bars of the piece: its sound reads `beat` or `bpm`, or a child its `children` name does, at any depth. */
function inTime(tool: string, seen = new Set<string>()): boolean {
  if (IN_TIME.includes(tool)) return true;
  if (seen.has(tool)) return false;
  seen.add(tool);
  const children: Record<string, string> = host.tools.get(tool)?.children ?? {};
  return host.readsBeat(tool) || Object.values(children).some((child) => inTime(child, seen));
}

/** The same of records: of children whose names only a record says. */
const anyInTime = (children: readonly { tool: string; children?: unknown }[]): boolean =>
  children.some((child) => inTime(child.tool) || anyInTime((child.children ?? []) as { tool: string }[]));

export interface TransportProps {
  /** A record button: whether it records, and what a press does. An action of the tool named `record` shows its key on it. */
  record?: { on: Accessor<boolean>; press: () => void };
  /** A switch for the computer keys, for a tool that notes play. */
  computerKeys?: boolean;
  /** A switch for writing automation, for a tool whose lanes its knobs write into. */
  writeAutomation?: boolean;
  /**
   * Whether it shows the time of the piece: stop, the bar and the beat, the time, the tempo, the
   * time signature and the metronome. When left out, a tool shows them that plays in the bars:
   * its sound reads `beat` or `bpm`, or it has a child in time at any depth, a timeline, a lane, a
   * region or a recorder. Say `time` for a tool whose code alone reads the clock.
   */
  time?: boolean;
}

/**
 * The transport of the piece, a pill for the `header` of a `Page`: play and pause, and the meter
 * of what the device plays. For a tool that plays in the bars, see `time`, also stop, where the
 * piece plays as a bar and a beat and as a time, the tempo and the time signature that play
 * there, which a drag up or down or a typed number changes as one undo step, and the metronome:
 * a page adds no tempo control of its own.
 * Space plays and pauses a tool that shows it, wherever the focus is: it gives the tool the
 * action `play`, unless the tool has its own.
 */
export function Transport({ record, computerKeys, writeAutomation, time }: TransportProps = {}): UiNode {
  const { clock, tool, children } = currentCard().drawn;
  const spec = host.tools.get(tool);
  if (spec && spec.actions?.play === undefined) spec.actions = { play: playPause, ...spec.actions };
  const timed = time ?? (inTime(tool) || untrack(() => anyInTime(children)));
  const pill: Style = {
    direction: "row", align: "center", gap: "md", height: 36, paddingLeft: "xs", paddingRight: "lg",
    radius: "full", background: "fill-strong", borderColor: "border-strong", borderWidth: 1, keep: true,
  };
  const row: Style = { direction: "row", align: "center" };
  const signature = () => signatureAt(clock.tempo_map.time_signatures, clock.tick);
  const setSignature = (beats: number, note: number) => {
    const { run } = signature();
    clock.update("Change the time signature", ({ tempo_map }) => {
      const changed = tempo_map.time_signatures[run];
      if (changed) changed.signature = `${beats}/${note}`;
      else tempo_map.time_signatures = [{ signature: `${beats}/${note}`, bars: 1 }];
    });
  };
  const setTempo = (bpm: number) => {
    const at = clock.tick;
    clock.update("Change the tempo", ({ tempo_map }) => {
      const change = tempo_map.tempo_changes.findLast((change) => change.tick <= at);
      if (change) change.bpm = bpm;
    });
  };
  const round = { variant: "ghost", size: "md", rounded: true } as const;
  return h("div", { key: "transport", style: pill },
    h("div", { style: { ...row, gap: "xs" } },
      Button({
        ...round, key: "play", icon: () => (clock.playing ? "pause" : "play"), variant: () => (clock.playing ? "subtle" : "ghost"), color: "green",
        tooltip: () => (clock.playing ? "Pause" : "Play"), shortcut: `${tool}.play`, onClick: () => (clock.playing ? clock.pause() : clock.play()),
      }),
      timed && Button({ ...round, key: "stop", icon: "square", tooltip: "Stop", onClick: () => clock.stop() }),
      record && Button({ ...round, key: "record", icon: "circle", variant: () => (record.on() ? "solid" : "ghost"), color: "red", tooltip: "Record", shortcut: `${tool}.record`, onClick: record.press }),
    ),
    timed && clockText("transport-position", 44, "position", "text"),
    timed && clockText("transport-time", 40, "time", "muted"),
    timed && h("div", { style: row },
      NumberBox({ key: "transport-tempo", width: 32, per: 2, min: 10, max: 1000, value: () => clock.bpm, onChange: setTempo, show: now.bpm.round() }),
      word("transport-bpm", 32, 0, "bpm"),
    ),
    timed && h("div", { style: row },
      NumberBox({ key: "transport-beats", width: 14, min: 1, max: 32, unit: "beats", value: () => signature().beats, onChange: (beats) => setSignature(beats, signature().note), show: now.tick }),
      word("transport-slash", 14, 3, "/"),
      NumberBox({ key: "transport-note", width: 14, values: NOTES, unit: "note", value: () => signature().note, onChange: (note) => setSignature(signature().beats, note), show: now.tick }),
    ),
    (computerKeys || writeAutomation || timed) && h("div", { style: { ...row, gap: "xs" } },
      computerKeys && Toggle({
        key: "computer-keys", size: "sm", icon: "keyboard-music", on: () => clock.computerKeys, tooltip: "Play notes with the computer keys", shortcut: "app.computer_keys",
        onClick: () => clock.setComputerKeys(!clock.computerKeys),
      }),
      // Red while on, as what records.
      writeAutomation && Toggle({
        key: "write-automation", size: "sm", icon: "automation", on: () => clock.writeAutomation, color: "red",
        tooltip: "Write automation: while it plays, moving a knob or a fader that has a lane records into the lane", onClick: () => clock.setWriteAutomation(!clock.writeAutomation),
      }),
      timed && Toggle({ key: "click", size: "sm", icon: "metronome", on: () => clock.click, tooltip: "Metronome", onClick: () => clock.setClick(!clock.click) }),
    ),
    // In a well, as a display: its bars show against it while nothing plays.
    h("div", { style: { justify: "center", height: 16, paddingX: "xs", radius: "sm", background: "well" } }, Meter({ key: "transport-meter", peaks: "device", horizontal: true })),
  );
}
