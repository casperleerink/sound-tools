import * as React from "react";
import type { ToolDefinition, ToolViewProps } from "@/core/sdk";
import { cn, Field, Knob, NumericInput, SegmentedControl, Slider, Switch } from "@/ui";

/* ------------------------------------------------------------------ fake data */

const RATIOS = [1, 2, 3, 4, 5, 6, 7, 8];

type Preset = "saw" | "square" | "odd" | "custom";

const SHAPES: Record<Exclude<Preset, "custom">, number[]> = {
  saw: RATIOS.map((n) => 1 / n),
  square: RATIOS.map((n) => (n % 2 ? 1 / n : 0)),
  odd: RATIOS.map((n) => (n % 2 ? 0.9 / Math.sqrt(n) : 0)),
};

/* --------------------------------------------------- shared state (both views) */

interface AdditiveState {
  partials: number[];
  preset: Preset;
  brightness: number;
  fundamental: number;
  detune: number;
  attack: number;
  decay: number;
  level: number;
  legato: boolean;
}

let state: AdditiveState = {
  partials: SHAPES.saw.map((v) => Number(v.toFixed(3))),
  preset: "saw",
  brightness: 62,
  fundamental: 220,
  detune: 7,
  attack: 18,
  decay: 480,
  level: -6,
  legato: true,
};

const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => listeners.delete(listener);
};
const getState = () => state;
function set(patch: Partial<AdditiveState>) {
  state = { ...state, ...patch };
  for (const listener of listeners) listener();
}
const useAdditive = () => React.useSyncExternalStore(subscribe, getState);

/* ------------------------------------------------------------------- formatting */

const signed = (n: number, digits = 0) => `${n > 0 ? "+" : n < 0 ? "−" : ""}${Math.abs(n).toFixed(digits)}`;

const NOTE_NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
function noteName(hz: number) {
  const midi = Math.round(12 * Math.log2(hz / 440) + 69);
  return `${NOTE_NAMES[((midi % 12) + 12) % 12]}${Math.floor(midi / 12) - 1}`;
}

/* ------------------------------------------------------------------ partials view
   Eight sliders and the spectrum presets. The voice parameters are in the Voice view. */

function PartialsView(_props: ToolViewProps) {
  const s = useAdditive();
  const [selected, setSelected] = React.useState(2);
  const [hovered, setHovered] = React.useState<number | null>(null);
  const active = hovered ?? selected;
  const activeHz = Math.round(s.fundamental * RATIOS[active]!);

  const setPartial = (index: number, value: number) => {
    const partials = s.partials.map((v, i) => (i === index ? Number(value.toFixed(3)) : v));
    set({ partials, preset: "custom" });
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="grid grid-cols-8 gap-2 pt-1">
        {s.partials.map((value, i) => {
          const on = i === active;
          return (
            <div
              key={RATIOS[i]}
              className="flex flex-col items-center gap-2"
              onPointerEnter={() => setHovered(i)}
              onPointerLeave={() => setHovered((h) => (h === i ? null : h))}
              onPointerDown={() => setSelected(i)}
              onFocus={() => setSelected(i)}
            >
              <div className="h-[96px]">
                <Slider
                  orientation="vertical"
                  size="sm"
                  value={value}
                  onChange={(v) => setPartial(i, v)}
                  min={0}
                  max={1}
                  label={`Partial ${RATIOS[i]} amplitude`}
                  aria-valuetext={`${Math.round(value * 100)} percent`}
                  className="h-full"
                />
              </div>
              <span className={cn("tabular text-2xs", on ? "text-gray-950" : "text-gray-700")}>{RATIOS[i]}</span>
            </div>
          );
        })}
      </div>

      <div className="flex items-center justify-between gap-3">
        <SegmentedControl
          size="xs"
          label="Spectrum preset"
          value={s.preset}
          onValueChange={(preset) => {
            if (preset === "custom") return set({ preset });
            set({ preset, partials: SHAPES[preset].map((v) => Number(v.toFixed(3))) });
          }}
          options={[
            { value: "saw", label: "Saw" },
            { value: "square", label: "Square" },
            { value: "odd", label: "Odd" },
            { value: "custom", label: "Custom" },
          ]}
        />
        <span className="tabular text-gray-700 text-xs">{activeHz} Hz</span>
      </div>
    </div>
  );
}

/* --------------------------------------------------------------------- voice view */

function VoiceView(_props: ToolViewProps) {
  const s = useAdditive();
  return (
    <div className="flex flex-col gap-4">
      <Field label="Fundamental" value={noteName(s.fundamental)}>
        <NumericInput value={s.fundamental} onChange={(fundamental) => set({ fundamental })} min={20} max={2000} precision={2} unit="Hz" block />
      </Field>
      <Field label="Brightness" layout="row" value={`${Math.round(s.brightness)} %`}>
        <Slider value={s.brightness} onChange={(brightness) => set({ brightness: Math.round(brightness) })} min={0} max={100} size="sm" />
      </Field>
      <Field label="Detune" layout="row" value={`${signed(s.detune)} ct`}>
        <Slider value={s.detune} onChange={(detune) => set({ detune: Math.round(detune) })} min={-50} max={50} step={1} bipolar size="sm" />
      </Field>
      <Field label="Level" layout="row" value={`${signed(s.level, 1)} dB`}>
        <Slider value={s.level} onChange={(level) => set({ level: Number(level.toFixed(1)) })} min={-60} max={6} size="sm" />
      </Field>
      <div className="flex items-center gap-6 pt-1">
        <KnobCell label="Attack" value={s.attack} min={0} max={200} unit="ms" onChange={(attack) => set({ attack })} />
        <KnobCell label="Decay" value={s.decay} min={0} max={1500} unit="ms" onChange={(decay) => set({ decay })} />
        <div className="ml-auto flex items-center gap-2">
          <span className="font-medium text-gray-900 text-xs">Legato</span>
          <Switch size="xs" label="Legato" checked={s.legato} onCheckedChange={(legato) => set({ legato })} />
        </div>
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
      <span className="tabular text-2xs text-gray-700">
        {Math.round(value)} {unit}
      </span>
    </div>
  );
}

export const tool: ToolDefinition = {
  type: "additive",
  name: "Additive",
  description: "Builds a voice from a fundamental and eight harmonic partials you mix by hand.",
  accent: "blue",
  inputs: [
    { id: "gate", label: "Gate", kind: "event" },
    { id: "pitch", label: "Pitch", kind: "mod" },
    { id: "brightness", label: "Brightness", kind: "mod" },
  ],
  outputs: [{ id: "out", label: "Out", kind: "audio" }],
  views: [
    { id: "partials", label: "Partials", width: 352, component: PartialsView },
    { id: "voice", label: "Voice", width: 320, component: VoiceView },
  ],
};
