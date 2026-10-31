import { Button, h, Label, Meter, NumberBox, now, Space, type Accessor, type Clock, type Color, type Frozen, type Live, type Style, type TempoMap } from "./sdk";

// The transport of the song, in the window's title bar, and the time of the tempo map.
const BEAT = 960;
/** The note values a time signature takes. */
const NOTES = [1, 2, 4, 8, 16, 32] as const;

type TempoChanges = readonly { tick: number; bpm: number }[];

/** Ticks as seconds at `bpm`. */
const secondsAt = (ticks: number, bpm: number) => (ticks / BEAT) * (60 / bpm);

/** The tempo at `tick`, and the tick of the next change after it. */
function tempoAt(changes: TempoChanges, tick: number): { bpm: number; next: number } {
  const bpm = [...changes].reverse().find((change) => change.tick <= tick)?.bpm ?? 120;
  return { bpm, next: changes.find((change) => change.tick > tick)?.tick ?? Infinity };
}

/** The seconds from `from` to `to`, through each tempo between: below 0 when `to` is before. */
export function secondsBetween(changes: TempoChanges, from: number, to: number): number {
  if (to < from) return -secondsBetween(changes, to, from);
  let seconds = 0;
  for (let tick = from; tick < to; ) {
    const { bpm, next } = tempoAt(changes, tick);
    const end = Math.min(next, to);
    seconds += secondsAt(end - tick, bpm);
    tick = end;
  }
  return seconds;
}

/** The ticks `seconds` take from `from`, through each tempo after it. */
export function ticksAcross(changes: TempoChanges, from: number, seconds: number): number {
  let [tick, left] = [from, seconds];
  while (left > 0) {
    const { bpm, next } = tempoAt(changes, tick);
    const fits = secondsAt(next - tick, bpm);
    if (left <= fits) return tick + (left * bpm * BEAT) / 60 - from;
    [tick, left] = [next, left - fits];
  }
  return tick - from;
}

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
function clockText(key: string, width: number, unit: "position" | "time", color: Color = "gray-950", text: Live = now.tick) {
  return <Space key={key} width={width} height={18}><Label x={0} y={0} text={text} unit={unit} color={color} size={14} /></Space>;
}

/** What the transport records with: whether it records, and what a press does. */
export interface TransportRecord {
  on: Accessor<boolean>;
  press: () => void;
}

/**
 * The window's transport, a pill: play and pause, stop, record, where the piece plays as a bar
 * and a beat and as a time, the tempo and the time signature that play at the playhead, which a drag up or down or a typed number changes as
 * one undo step, the computer keys, the click and the meter of what the device plays.
 */
export function Transport({ clock, record }: { clock: Clock; record: TransportRecord }) {
  const pill: Style = {
    direction: "row", align: "center", gap: 12, height: 36, paddingLeft: 4, paddingRight: 16,
    radius: 18, background: "#292b30", borderColor: "alpha/10", borderWidth: 1, keep: true,
  };
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
  const toggle = (on: Accessor<boolean>) => ({
    variant: () => (on() ? "primary" : "ghost") as "primary" | "ghost",
    color: () => (on() ? undefined : "gray-700") as Color,
  });
  return (
    <div key="transport" style={pill}>
      <div style={{ direction: "row", align: "center", gap: 4 }}>
        <Button key="play" icon={() => (clock.playing ? "pause" : "play")} variant={() => (clock.playing ? "subtle" : "ghost")} color="green" size="sm" rounded
          tooltip={() => (clock.playing ? "Pause" : "Play")} shortcut="space" onClick={() => (clock.playing ? clock.pause() : clock.play())} />
        <Button key="stop" icon="square" variant="ghost" size="sm" rounded tooltip="Stop" onClick={() => clock.stop()} />
        <Button key="record" icon="circle" variant={() => (record.on() ? "solid" : "ghost")} color="red" size="sm" rounded tooltip="Record" shortcut="r" onClick={record.press} />
      </div>
      {clockText("transport-position", 28, "position")}
      {clockText("transport-time", 34, "time", "gray-700")}
      <div style={{ direction: "row", align: "center" }}>
        <NumberBox key="transport-tempo" width={32} per={2} min={10} max={1000}
          value={() => clock.bpm} onChange={setTempo} show={now.bpm.round()} />
        <Space key="transport-bpm" width={32} height={18}><Label x={0} y={2} text="bpm" color="gray-700" size={12} /></Space>
      </div>
      <div style={{ direction: "row", align: "center" }}>
        <NumberBox key="transport-beats" width={14} min={1} max={32} unit="beats"
          value={() => signature().beats} onChange={(beats) => setSignature(beats, signature().note)} show={now.tick} />
        <Space key="transport-slash" width={14} height={18}><Label x={3} y={2} text="/" color="gray-700" size={12} /></Space>
        <NumberBox key="transport-note" width={14} values={NOTES} unit="note"
          value={() => signature().note} onChange={(note) => setSignature(signature().beats, note)} show={now.tick} />
      </div>
      <Button key="computer-keys" icon="keyboard-music" {...toggle(() => clock.computerKeys)} size="sm" rounded tooltip="Play notes with the computer keys" shortcut="cmd-k"
        onClick={() => clock.setComputerKeys(!clock.computerKeys)} />
      <Button key="click" icon="metronome" {...toggle(() => clock.click)} size="sm" rounded tooltip="Metronome" onClick={() => clock.setClick(!clock.click)} />
      <Meter key="transport-meter" peaks="device" horizontal />
    </div>
  );
}
