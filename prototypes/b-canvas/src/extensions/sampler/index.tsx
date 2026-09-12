import { FolderOpen } from "lucide-react";
import * as React from "react";
import type { ToolDefinition, ToolViewProps } from "@/core/sdk";
import { Badge, cn, Field, IconButton, Knob, NumericInput, SegmentedControl, Separator, Slider, Switch } from "@/ui";

/* ------------------------------------------------------------------ fake data */

const SAMPLE = {
  folder: "assets/",
  file: "field-recording.wav",
  duration: 42.3,
  rate: "48 kHz",
  channels: "stereo",
};

/** Deterministic stereo-summed peaks: a slow swell with grain and a few transients. */
const PEAK_COUNT = 132;
const PEAKS: number[] = (() => {
  let seed = 0x5eed1234;
  const rnd = () => {
    seed = (seed * 1664525 + 1013904223) >>> 0;
    return seed / 0x100000000;
  };
  return Array.from({ length: PEAK_COUNT }, (_, i) => {
    const t = i / (PEAK_COUNT - 1);
    const body = 0.36 + 0.4 * Math.sin(Math.PI * t) + 0.16 * Math.sin(t * 13.2 + 1.1);
    const grain = 0.55 + 0.45 * rnd() ** 1.3;
    const transient = rnd() > 0.955 ? 0.3 + 0.25 * rnd() : 0;
    const fade = t < 0.04 ? t / 0.04 : t > 0.93 ? (1 - t) / 0.07 : 1;
    return Math.max(0.05, Math.min(1, (body * grain + transient) * fade));
  });
})();

/* --------------------------------------------------- shared state (both views) */

type PlayMode = "oneshot" | "loop";

interface SamplerState {
  start: number;
  end: number;
  playhead: number;
  mode: PlayMode;
  trim: boolean;
  pitch: number;
  gain: number;
  attack: number;
  release: number;
  root: number;
}

let state: SamplerState = {
  start: 6.4,
  end: 23.8,
  playhead: 12.75,
  mode: "loop",
  trim: true,
  pitch: 7,
  gain: -4.5,
  attack: 24,
  release: 320,
  root: 261.63,
};

const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => listeners.delete(listener);
};
const getState = () => state;
function set(patch: Partial<SamplerState>) {
  state = { ...state, ...patch };
  for (const listener of listeners) listener();
}
const useSampler = () => React.useSyncExternalStore(subscribe, getState);

/* ------------------------------------------------------------------- formatting */

const clock = (t: number) => `${Math.floor(t / 60)}:${Math.floor(t % 60).toString().padStart(2, "0")}`;
const clockTenths = (t: number) => `${clock(t)}.${Math.floor((t % 60) % 1 * 10)}`;
const signed = (n: number, digits = 0) => `${n > 0 ? "+" : n < 0 ? "−" : ""}${Math.abs(n).toFixed(digits)}`;

const NOTE_NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
function noteName(hz: number) {
  const midi = Math.round(12 * Math.log2(hz / 440) + 69);
  return `${NOTE_NAMES[((midi % 12) + 12) % 12]}${Math.floor(midi / 12) - 1}`;
}

/* ------------------------------------------------------------------ waveform view */

const W = 356;
const H = 120;
const PAD = 10;
const SPAN = W - PAD * 2;
const CENTER = 70;
const AMP = 40;
const WAVE_TOP = 24;
const WAVE_BOT = 114;
const RULER = [0, 10, 20, 30, 40];

const xOf = (t: number) => PAD + (t / SAMPLE.duration) * SPAN;
const tOf = (x: number) => ((x - PAD) / SPAN) * SAMPLE.duration;
const clampTime = (t: number) => Math.min(SAMPLE.duration, Math.max(0, t));

function WaveformView(_props: ToolViewProps) {
  const s = useSampler();
  const svgRef = React.useRef<SVGSVGElement>(null);
  const drag = React.useRef<"start" | "end" | "playhead" | null>(null);

  const timeAt = (e: React.PointerEvent) => {
    const rect = svgRef.current?.getBoundingClientRect();
    if (!rect) return 0;
    return clampTime(tOf(((e.clientX - rect.left) / rect.width) * W));
  };

  const begin = (kind: "start" | "end" | "playhead") => (e: React.PointerEvent<SVGGElement | SVGRectElement>) => {
    drag.current = kind;
    e.currentTarget.setPointerCapture(e.pointerId);
    if (kind === "playhead") set({ playhead: timeAt(e) });
  };
  const move = (e: React.PointerEvent) => {
    const kind = drag.current;
    if (!kind) return;
    const t = timeAt(e);
    if (kind === "start") set({ start: Math.min(t, s.end - 0.5) });
    else if (kind === "end") set({ end: Math.max(t, s.start + 0.5) });
    else set({ playhead: t });
  };
  const end = () => {
    drag.current = null;
  };

  const startX = xOf(s.start);
  const endX = xOf(s.end);
  const playX = xOf(s.playhead);

  return (
    <div className="flex flex-col gap-2">
      {/* file */}
      <div className="flex items-center gap-2">
        <span className="truncate font-mono text-gray-900 text-xs">
          <span className="text-gray-700">{SAMPLE.folder}</span>
          {SAMPLE.file}
        </span>
        <Badge variant="muted" size="xs">
          {SAMPLE.rate} &middot; {SAMPLE.channels}
        </Badge>
        <span className="flex-1" />
        <IconButton label="Load sample" size="xs" variant="ghost-muted">
          <FolderOpen />
        </IconButton>
      </div>

      {/* the one custom area: waveform, region handles, playhead */}
      <div className="relative h-[120px] overflow-hidden rounded-lg bg-gray-100">
        <svg
          ref={svgRef}
          viewBox={`0 0 ${W} ${H}`}
          preserveAspectRatio="none"
          className="absolute inset-0 h-full w-full"
          onPointerMove={move}
          onPointerUp={end}
          onPointerCancel={end}
        >
          <title>Waveform of {SAMPLE.file}</title>
          <rect x={0} y={0} width={W} height={H} fill="transparent" className="cursor-ew-resize" onPointerDown={begin("playhead")} />
          {RULER.map((t) => (
            <line
              key={t}
              x1={xOf(t)}
              x2={xOf(t)}
              y1={WAVE_TOP}
              y2={WAVE_BOT}
              stroke="var(--color-gray-400)"
              strokeOpacity={0.5}
              strokeWidth={1}
            />
          ))}
          <rect x={startX} y={WAVE_TOP} width={Math.max(0, endX - startX)} height={WAVE_BOT - WAVE_TOP} fill="var(--accent)" fillOpacity={0.08} />
          <line x1={PAD} x2={W - PAD} y1={CENTER} y2={CENTER} stroke="var(--color-gray-400)" strokeWidth={1} />
          {PEAKS.map((peak, i) => {
            const x = PAD + ((i + 0.5) / PEAK_COUNT) * SPAN;
            const inside = x >= startX && x <= endX;
            const h = Math.max(0.8, peak * AMP);
            return (
              <rect
                key={i}
                x={x - 0.75}
                y={CENTER - h}
                width={1.5}
                height={h * 2}
                rx={0.75}
                fill={inside ? "var(--accent)" : "var(--color-gray-600)"}
              />
            );
          })}
          {RULER.map((t) => (
            <line key={`tick-${t}`} x1={xOf(t)} x2={xOf(t)} y1={WAVE_BOT + 1} y2={WAVE_BOT + 5} stroke="var(--color-gray-500)" strokeWidth={1} />
          ))}
          {/* playhead */}
          <line x1={playX} x2={playX} y1={WAVE_TOP - 4} y2={WAVE_BOT} stroke="var(--color-gray-950)" strokeWidth={1} />
          {/* region handles */}
          {(["start", "end"] as const).map((kind) => {
            const x = kind === "start" ? startX : endX;
            return (
              <g
                key={kind}
                role="slider"
                aria-label={kind === "start" ? "Region start" : "Region end"}
                aria-valuemin={0}
                aria-valuemax={SAMPLE.duration}
                aria-valuenow={kind === "start" ? s.start : s.end}
                aria-valuetext={`${(kind === "start" ? s.start : s.end).toFixed(2)} seconds`}
                tabIndex={0}
                className="cursor-ew-resize"
                onPointerDown={begin(kind)}
                onPointerMove={move}
                onPointerUp={end}
                onPointerCancel={end}
              >
                <rect x={x - 7} y={WAVE_TOP - 8} width={14} height={WAVE_BOT - WAVE_TOP + 8} fill="transparent" />
                <line x1={x} y1={WAVE_TOP - 6} x2={x} y2={WAVE_BOT} stroke="var(--accent)" strokeWidth={1.5} />
                <rect x={kind === "start" ? x : x - 6} y={WAVE_TOP - 8} width={6} height={9} rx={1.5} fill="var(--accent)" />
              </g>
            );
          })}
        </svg>
        <span
          className="tabular pointer-events-none absolute top-1 -translate-x-1/2 rounded bg-gray-300 px-1 py-px text-2xs text-gray-950"
          style={{ left: `${(playX / W) * 100}%` }}
        >
          {clockTenths(s.playhead)}
        </span>
      </div>

      {/* ruler */}
      <div className="relative h-3.5">
        {RULER.map((t, i) => (
          <span
            key={t}
            className={cn("tabular absolute top-0 text-2xs text-gray-700", i === 0 ? "" : "-translate-x-1/2")}
            style={{ left: `${(xOf(t) / W) * 100}%` }}
          >
            {clock(t)}
          </span>
        ))}
      </div>

      {/* transport parameters */}
      <div className="flex items-start gap-3">
        <Field label="Start" className="w-[76px]">
          <NumericInput value={s.start} onChange={(v) => set({ start: Math.min(v, s.end - 0.5) })} min={0} max={SAMPLE.duration} precision={2} unit="s" block />
        </Field>
        <Field label="End" className="w-[76px]">
          <NumericInput value={s.end} onChange={(v) => set({ end: Math.max(v, s.start + 0.5) })} min={0} max={SAMPLE.duration} precision={2} unit="s" block />
        </Field>
        <Field label="Play mode">
          <SegmentedControl
            label="Play mode"
            value={s.mode}
            onValueChange={(mode) => set({ mode })}
            options={[
              { value: "oneshot", label: "One-shot" },
              { value: "loop", label: "Loop" },
            ]}
          />
        </Field>
        <Field label="Trim" className="ml-auto">
          <div className="flex h-7 items-center">
            <Switch size="xs" accent label="Trim silence" checked={s.trim} onCheckedChange={(trim) => set({ trim })} />
          </div>
        </Field>
      </div>
    </div>
  );
}

/* --------------------------------------------------------------------- voice view */

function VoiceView(_props: ToolViewProps) {
  const s = useSampler();
  return (
    <div className="flex flex-col gap-3">
      <Field label="Pitch" value={`${signed(s.pitch)} st`}>
        <Slider value={s.pitch} onChange={(pitch) => set({ pitch: Math.round(pitch) })} min={-24} max={24} step={1} bipolar size="sm" />
      </Field>
      <Field label="Gain" value={`${signed(s.gain, 1)} dB`}>
        <Slider value={s.gain} onChange={(gain) => set({ gain: Number(gain.toFixed(1)) })} min={-60} max={6} size="sm" />
      </Field>
      <Separator />
      <div className="flex items-center gap-4">
        <KnobCell label="Attack" value={s.attack} min={0} max={250} unit="ms" onChange={(attack) => set({ attack })} />
        <KnobCell label="Release" value={s.release} min={0} max={1200} unit="ms" onChange={(release) => set({ release })} />
        <Field label="Root" value={noteName(s.root)} className="ml-auto w-[132px]">
          <div className="flex items-center gap-2">
            <NumericInput value={s.root} onChange={(root) => set({ root })} min={20} max={4000} precision={2} unit="Hz" block />
          </div>
        </Field>
      </div>
    </div>
  );
}

function KnobCell({
  label,
  value,
  min,
  max,
  unit,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  unit: string;
  onChange: (v: number) => void;
}) {
  return (
    <div className="flex flex-col items-center gap-1.5">
      <Knob value={value} onChange={(v) => onChange(Math.round(v))} min={min} max={max} size={36} label={label} />
      <span className="font-medium text-gray-900 text-xs">{label}</span>
      <span className="tabular text-2xs text-gray-800">
        {Math.round(value)} {unit}
      </span>
    </div>
  );
}

export const tool: ToolDefinition = {
  type: "sampler",
  name: "Sampler",
  description: "Plays a region of a loaded sample as a one-shot or a loop, with pitch and envelope per voice.",
  accent: "yellow",
  inputs: [
    { id: "trigger", label: "Trigger", kind: "event" },
    { id: "pitch", label: "Pitch", kind: "mod" },
  ],
  outputs: [{ id: "out", label: "Out", kind: "audio" }],
  views: [
    { id: "wave", label: "Waveform", width: 380, component: WaveformView },
    { id: "voice", label: "Voice", width: 300, component: VoiceView },
  ],
};
