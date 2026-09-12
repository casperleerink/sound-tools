import { RotateCcw } from "lucide-react";
import {
  IconButton,
  LabelledControl,
  Meter,
  NumericInput,
  SegmentedControl,
  Slider,
  Switch,
  ToolView,
  ToolViewBody,
  useInstanceState,
} from "@/sdk";
import type { ExtensionDef, ToolViewProps } from "@/sdk";

type Wave = "sine" | "triangle" | "saw";

interface Preset {
  freq: number;
  level: number;
  attack: number;
  release: number;
  wave: Wave;
  followPitch: boolean;
}

const DEFAULT_PRESET: Preset = {
  freq: 220,
  level: -6,
  attack: 8,
  release: 600,
  wave: "sine",
  followPitch: true,
};

/** Each Tone instance starts somewhere else so the workspace is not three copies. */
const PRESETS: Record<string, Preset | undefined> = {
  "tone-a": DEFAULT_PRESET,
  "tone-b": { freq: 165, level: -12, attack: 120, release: 1800, wave: "triangle", followPitch: false },
  "tone-c": { freq: 330, level: -9, attack: 2, release: 320, wave: "sine", followPitch: false },
};

function presetFor(instanceId: string): Preset {
  return PRESETS[instanceId] ?? DEFAULT_PRESET;
}

const WAVES: { value: Wave; label: string }[] = [
  { value: "sine", label: "Sine" },
  { value: "triangle", label: "Tri" },
  { value: "saw", label: "Saw" },
];

const VIEW_W = 200;
const VIEW_H = 48;
const MID = VIEW_H / 2;

/** One cycle of the chosen wave, amplitude follows the level. */
function wavePath(wave: Wave, amp: number): string {
  const y = (u: number) => (MID - u * amp).toFixed(2);
  if (wave === "triangle") {
    return `M 0 ${y(0)} L 50 ${y(1)} L 150 ${y(-1)} L 200 ${y(0)}`;
  }
  if (wave === "saw") {
    return `M 0 ${y(0)} L 100 ${y(1)} L 100 ${y(-1)} L 200 ${y(0)}`;
  }
  const steps = 48;
  let d = "";
  for (let i = 0; i <= steps; i++) {
    const t = i / steps;
    d += `${i === 0 ? "M" : "L"} ${(t * VIEW_W).toFixed(1)} ${y(Math.sin(t * 2 * Math.PI))} `;
  }
  return d.trim();
}

function ToneMain({ instance }: ToolViewProps) {
  const preset = presetFor(instance.id);
  const [freq, setFreq] = useInstanceState(instance.id, "freq", preset.freq);
  const [level, setLevel] = useInstanceState(instance.id, "level", preset.level);
  const [attack, setAttack] = useInstanceState(instance.id, "attack", preset.attack);
  const [release, setRelease] = useInstanceState(instance.id, "release", preset.release);
  const [wave, setWave] = useInstanceState<Wave>(instance.id, "wave", preset.wave);
  const [followPitch, setFollowPitch] = useInstanceState(instance.id, "followPitch", preset.followPitch);

  const reset = () => {
    setFreq(preset.freq);
    setLevel(preset.level);
    setAttack(preset.attack);
    setRelease(preset.release);
    setWave(preset.wave);
    setFollowPitch(preset.followPitch);
  };

  const amp = 4 + 14 * 10 ** (level / 20);

  return (
    <ToolView
      title={instance.name}
      meta={`${freq.toFixed(1)} Hz · ${wave}`}
      actions={
        <IconButton size="xs" variant="quiet" label="Reset to saved values" onClick={reset}>
          <RotateCcw />
        </IconButton>
      }
    >
      <ToolViewBody className="@container gap-3">
        <div className="grid grid-cols-1 gap-x-6 gap-y-4 @md:grid-cols-2">
          <LabelledControl label="Frequency" htmlFor={`${instance.id}-freq`}>
            <div className="flex items-center gap-2">
              <Slider
                id={`${instance.id}-freq`}
                label="Frequency"
                value={freq}
                onChange={setFreq}
                min={20}
                max={2000}
                step={0.1}
                disabled={followPitch}
              />
              <NumericInput
                label="Frequency"
                value={freq}
                onChange={setFreq}
                min={20}
                max={2000}
                step={0.1}
                unit="Hz"
                size="xs"
                className="w-22"
                disabled={followPitch}
              />
            </div>
          </LabelledControl>

          <LabelledControl label="Level" htmlFor={`${instance.id}-level`}>
            <div className="flex items-center gap-2">
              <Slider
                id={`${instance.id}-level`}
                label="Level"
                value={level}
                onChange={setLevel}
                min={-60}
                max={0}
                step={0.5}
              />
              <NumericInput
                label="Level"
                value={level}
                onChange={setLevel}
                min={-60}
                max={0}
                step={0.5}
                unit="dB"
                size="xs"
                className="w-20"
              />
            </div>
          </LabelledControl>

          <LabelledControl label="Attack" htmlFor={`${instance.id}-attack`}>
            <div className="flex items-center gap-2">
              <Slider
                id={`${instance.id}-attack`}
                label="Attack"
                value={attack}
                onChange={setAttack}
                min={0}
                max={2000}
                step={1}
              />
              <NumericInput
                label="Attack"
                value={attack}
                onChange={setAttack}
                min={0}
                max={2000}
                step={1}
                unit="ms"
                size="xs"
                className="w-18"
              />
            </div>
          </LabelledControl>

          <LabelledControl label="Release" htmlFor={`${instance.id}-release`}>
            <div className="flex items-center gap-2">
              <Slider
                id={`${instance.id}-release`}
                label="Release"
                value={release}
                onChange={setRelease}
                min={0}
                max={4000}
                step={1}
              />
              <NumericInput
                label="Release"
                value={release}
                onChange={setRelease}
                min={0}
                max={4000}
                step={1}
                unit="ms"
                size="xs"
                className="w-18"
              />
            </div>
          </LabelledControl>
        </div>

        <div className="grid grid-cols-1 gap-x-6 gap-y-1 @md:grid-cols-2">
          <LabelledControl label="Waveform" layout="row">
            <SegmentedControl aria-label="Waveform" value={wave} onValueChange={setWave} options={WAVES} />
          </LabelledControl>
          <LabelledControl label="Follow pitch input" layout="row">
            <Switch label="Follow pitch input" checked={followPitch} onCheckedChange={setFollowPitch} />
          </LabelledControl>
        </div>

        <div className="flex flex-col gap-2 border-t border-alpha/5 pt-3">
          <svg
            viewBox={`0 0 ${VIEW_W} ${VIEW_H}`}
            preserveAspectRatio="none"
            className="h-12 w-full"
            aria-hidden
          >
            <line
              x1={0}
              y1={MID}
              x2={VIEW_W}
              y2={MID}
              stroke="var(--color-alpha)"
              strokeOpacity={0.1}
              vectorEffect="non-scaling-stroke"
            />
            <path
              d={wavePath(wave, amp)}
              fill="none"
              stroke="var(--accent)"
              strokeWidth={1.5}
              strokeLinecap="round"
              strokeLinejoin="round"
              vectorEffect="non-scaling-stroke"
            />
          </svg>
          <div className="flex items-center gap-2">
            <span className="shrink-0 text-2xs font-semibold uppercase tracking-[0.08em] text-gray-950/40">
              out
            </span>
            <Meter label="Output level" orientation="horizontal" level={-14} peak={-9} thickness={6} scale />
            <span className="shrink-0 text-xs text-gray-950/60 tabular">−14.0 dB</span>
          </div>
        </div>
      </ToolViewBody>
    </ToolView>
  );
}

export const toneExtension: ExtensionDef = {
  name: "tone",
  title: "Tone",
  description: "A single-voice sine, triangle or saw oscillator with an attack/release envelope.",
  tools: [
    {
      id: "tone",
      name: "Tone",
      kind: "instrument",
      accent: "teal",
      ports: [
        { id: "trigger", label: "trigger", kind: "event", direction: "in" },
        { id: "pitch", label: "pitch", kind: "modulation", direction: "in" },
        { id: "out", label: "out", kind: "audio", direction: "out" },
      ],
      views: [{ id: "main", label: "Main", component: ToneMain }],
    },
  ],
};
