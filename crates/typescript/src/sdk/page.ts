// The parts of a view that fills the window: the page, the panels on it and the transport.

import { Meter, NumberBox } from "./components";
import { Button, Color, h, Label, Space, Style, Text } from "./drawing";
import { Clock, Live, now, TempoMap } from "./live";
import { Child, currentCard, element, UiNode } from "./nodes";
import { Accessor, Bind } from "./reactivity";
import { Action, Frozen } from "./tools";

export type PageProps = {
  /**
   * What it shows in the window's title bar, side by side in the middle of the room right of the
   * project menu: a `Transport`, the tool's own controls. A page that is not the window's shows none.
   */
  header?: Child;
  children?: Child;
} & (
  | {
      /** The widest its content gets, in points, in the middle of the window: 720 reads well for text. As wide as the window when left out. */
      maxWidth?: number;
      flush?: never;
    }
  | {
      /** Its content reaches the edges of the window and does not scroll: one surface, such as a canvas or a timeline. */
      flush: true;
      maxWidth?: never;
    }
);

/**
 * The top of a view that fills the window: its content one under the other with the room of the
 * app around and between, scrolling down where the window is too small for it. A part with
 * `grow` takes the room that is left, down to what it needs: a `Space` its `height`.
 */
export function Page({ header, children, flush, maxWidth }: PageProps): UiNode {
  const bar = header === undefined ? null : element("div", { titleBar: true, style: { direction: "row", align: "center", gap: "md" } }, [header]);
  if (flush) return h("div", { style: { grow: true } }, bar, children);
  // A row, so the content grows to `maxWidth` across and sits in the middle of what is left.
  const body: Style = { grow: true, scroll: "y", direction: "row", justify: "center", padding: "xl" };
  return h("div", { style: { grow: true } }, bar, h("div", { style: body }, h("div", { style: { grow: true, maxWidth, gap: "lg" } }, children)));
}

export interface PanelProps {
  /** Names it for tests, `div-<key>`. */
  key?: string;
  title?: Bind<string>;
  /** Takes the room left in the page, or in the div it is in, with the other parts that grow. */
  grow?: boolean;
  /** In points, for one beside a panel that grows, such as a list: as wide as its content when left out. */
  width?: number;
  children?: Child;
}

/** A surface on a page, as a card looks, its content one under the other: side by side in a `div` that is a row. */
export const Panel = ({ key, title, grow, width, children }: PanelProps): UiNode =>
  h("div", { key, style: { grow, width, keep: width !== undefined, gap: "md", padding: "lg", radius: "lg", background: "surface", borderColor: "border", borderWidth: 1 } },
    title === undefined ? null : Text({ variant: "heading", children: title }), children);

/** Space plays and pauses the piece, wherever the focus is: `actions: { play: playPause }`. Named `play`, its key shows on the transport's button. */
export const playPause: Action<{ clock: Clock }> = {
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
const clockText = (key: string, width: number, unit: "position" | "time", color: Color, text: Live = now.tick) =>
  Space({ key, width, height: 18, children: Label({ x: 0, y: 0, text, unit, color, size: 14 }) });

/** A word of the pill `x` points into its room, such as the unit of the number before it. */
const word = (key: string, width: number, x: number, text: string) =>
  Space({ key, width, height: 18, children: Label({ x, y: 2, text, color: "text-muted", size: 12 }) });

export interface TransportProps {
  /** A record button: whether it records, and what a press does. An action of the tool named `record` shows its key on it. */
  record?: { on: Accessor<boolean>; press: () => void };
  /** A switch for the computer keys, for a tool that notes play. */
  computerKeys?: boolean;
  /** A switch for writing automation, for a tool whose lanes its knobs write into. */
  writeAutomation?: boolean;
}

/**
 * The transport of the piece, a pill for the `header` of a `Page`: play and pause, stop, where
 * the piece plays as a bar and a beat and as a time, the tempo and the time signature that play
 * there, which a drag up or down or a typed number changes as one undo step, the click and the
 * meter of what the device plays.
 */
export function Transport({ record, computerKeys, writeAutomation }: TransportProps = {}): UiNode {
  const { clock, tool } = currentCard().drawn;
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
  /** A switch of the pill: bright while it is on. */
  const toggle = (on: Accessor<boolean>) => ({
    ...round,
    variant: () => (on() ? "primary" : "ghost") as "primary" | "ghost",
    color: () => (on() ? undefined : "text-muted") as Color,
  });
  return h("div", { key: "transport", style: pill },
    h("div", { style: { ...row, gap: "xs" } },
      Button({
        ...round, key: "play", icon: () => (clock.playing ? "pause" : "play"), variant: () => (clock.playing ? "subtle" : "ghost"), color: "green",
        tooltip: () => (clock.playing ? "Pause" : "Play"), shortcut: `${tool}.play`, onClick: () => (clock.playing ? clock.pause() : clock.play()),
      }),
      Button({ ...round, key: "stop", icon: "square", tooltip: "Stop", onClick: () => clock.stop() }),
      record && Button({ ...round, key: "record", icon: "circle", variant: () => (record.on() ? "solid" : "ghost"), color: "red", tooltip: "Record", shortcut: `${tool}.record`, onClick: record.press }),
    ),
    clockText("transport-position", 44, "position", "text"),
    clockText("transport-time", 40, "time", "text-muted"),
    h("div", { style: row },
      NumberBox({ key: "transport-tempo", width: 32, per: 2, min: 10, max: 1000, value: () => clock.bpm, onChange: setTempo, show: now.bpm.round() }),
      word("transport-bpm", 32, 0, "bpm"),
    ),
    h("div", { style: row },
      NumberBox({ key: "transport-beats", width: 14, min: 1, max: 32, unit: "beats", value: () => signature().beats, onChange: (beats) => setSignature(beats, signature().note), show: now.tick }),
      word("transport-slash", 14, 3, "/"),
      NumberBox({ key: "transport-note", width: 14, values: NOTES, unit: "note", value: () => signature().note, onChange: (note) => setSignature(signature().beats, note), show: now.tick }),
    ),
    computerKeys && Button({
      ...toggle(() => clock.computerKeys), key: "computer-keys", icon: "keyboard-music", tooltip: "Play notes with the computer keys", shortcut: "app.computer_keys",
      onClick: () => clock.setComputerKeys(!clock.computerKeys),
    }),
    writeAutomation && Button({
      ...toggle(() => clock.writeAutomation), key: "write-automation", icon: "automation",
      tooltip: "Write automation: while it plays, moving a knob or a fader that has a lane records into the lane", onClick: () => clock.setWriteAutomation(!clock.writeAutomation),
    }),
    Button({ ...toggle(() => clock.click), key: "click", icon: "metronome", tooltip: "Metronome", onClick: () => clock.setClick(!clock.click) }),
    Meter({ key: "transport-meter", peaks: "device", horizontal: true }),
  );
}
