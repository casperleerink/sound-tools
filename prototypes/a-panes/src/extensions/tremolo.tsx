import { RotateCcw } from "lucide-react";
import {
  IconButton,
  Knob,
  LabelledControl,
  Meter,
  NumericInput,
  SegmentedControl,
  Select,
  Switch,
  ToolView,
  ToolViewBody,
  useInstanceState,
} from "@/sdk";
import type { ExtensionDef, Port, ToolViewProps } from "@/sdk";

type Shape = "sine" | "triangle" | "square";
type Division = "1/4" | "1/8" | "1/8T" | "1/16";

const DEFAULTS = {
  rate: 4.5,
  depth: 60,
  mix: 100,
  shape: "sine" as Shape,
  sync: false,
  division: "1/8" as Division,
};

const SHAPES: { value: Shape; label: string; ariaLabel: string }[] = [
  { value: "sine", label: "Sine", ariaLabel: "Sine" },
  { value: "triangle", label: "Tri", ariaLabel: "Triangle" },
  { value: "square", label: "Sqr", ariaLabel: "Square" },
];

const DIVISIONS: { value: Division; label: string }[] = [
  { value: "1/4", label: "1/4" },
  { value: "1/8", label: "1/8" },
  { value: "1/8T", label: "1/8T" },
  { value: "1/16", label: "1/16" },
];

const VIEW_W = 200;
const VIEW_H = 48;
const MID = VIEW_H / 2;
const CYCLES = 2;
const CYCLE_W = VIEW_W / CYCLES;

/** Two cycles of the LFO shape. `amp` is half the height of the swing in px. */
function lfoPath(shape: Shape, amp: number): string {
  const y = (u: number) => (MID - u * amp).toFixed(2);
  const parts: string[] = [];

  if (shape === "sine") {
    const steps = 64;
    for (let i = 0; i <= steps; i++) {
      const t = i / steps;
      parts.push(`${i === 0 ? "M" : "L"} ${(t * VIEW_W).toFixed(1)} ${y(Math.sin(t * 2 * Math.PI * CYCLES))}`);
    }
    return parts.join(" ");
  }

  for (let c = 0; c < CYCLES; c++) {
    const x = c * CYCLE_W;
    if (shape === "triangle") {
      if (c === 0) parts.push(`M ${x} ${y(0)}`);
      parts.push(`L ${x + CYCLE_W * 0.25} ${y(1)}`);
      parts.push(`L ${x + CYCLE_W * 0.75} ${y(-1)}`);
      parts.push(`L ${x + CYCLE_W} ${y(0)}`);
    } else {
      // square: flat top, drop, flat bottom, rise
      if (c === 0) parts.push(`M ${x} ${y(1)}`);
      parts.push(`L ${x + CYCLE_W * 0.5} ${y(1)}`);
      parts.push(`L ${x + CYCLE_W * 0.5} ${y(-1)}`);
      parts.push(`L ${x + CYCLE_W} ${y(-1)}`);
      parts.push(`L ${x + CYCLE_W} ${y(1)}`);
    }
  }
  return parts.join(" ");
}

/** One knob with an editable number under it. Both carry the same accessible label. */
function KnobCell({
  id,
  label,
  value,
  onChange,
  min,
  max,
  step,
  unit,
}: {
  id: string;
  label: string;
  value: number;
  onChange: (v: number) => void;
  min: number;
  max: number;
  step: number;
  unit: string;
}) {
  return (
    <div className="flex flex-col items-start gap-2">
      <Knob id={id} label={label} value={value} onChange={onChange} min={min} max={max} step={step} size={44} />
      <NumericInput
        label={label}
        value={value}
        onChange={onChange}
        min={min}
        max={max}
        step={step}
        unit={unit}
        size="xs"
        className="w-full"
      />
    </div>
  );
}

function TremoloMain({ instance }: ToolViewProps) {
  const [rate, setRate] = useInstanceState(instance.id, "rate", DEFAULTS.rate);
  const [depth, setDepth] = useInstanceState(instance.id, "depth", DEFAULTS.depth);
  const [mix, setMix] = useInstanceState(instance.id, "mix", DEFAULTS.mix);
  const [shape, setShape] = useInstanceState<Shape>(instance.id, "shape", DEFAULTS.shape);
  const [sync, setSync] = useInstanceState(instance.id, "sync", DEFAULTS.sync);
  const [division, setDivision] = useInstanceState<Division>(instance.id, "division", DEFAULTS.division);

  const reset = () => {
    setRate(DEFAULTS.rate);
    setDepth(DEFAULTS.depth);
    setMix(DEFAULTS.mix);
    setShape(DEFAULTS.shape);
    setSync(DEFAULTS.sync);
    setDivision(DEFAULTS.division);
  };

  // 0 % depth draws a flat line, 100 % fills most of the strip.
  const amp = (depth / 100) * (MID - 4);
  const rateText = sync ? division : `${rate.toFixed(1)} Hz`;

  return (
    <ToolView
      title={instance.name}
      meta={`${rateText} · ${shape} · ${depth.toFixed(0)} %`}
      actions={
        <IconButton size="xs" variant="quiet" label="Reset to saved values" onClick={reset}>
          <RotateCcw />
        </IconButton>
      }
    >
      <ToolViewBody className="@container gap-4">
        {/* Main parameters: one knob per column, editable number under each. */}
        <div className="grid grid-cols-3 gap-x-3 gap-y-2">
          <LabelledControl label="Rate" htmlFor={`${instance.id}-rate`}>
            {sync ? (
              <span className="flex h-11 items-center">
                <Select
                  id={`${instance.id}-rate`}
                  label="Rate division"
                  value={division}
                  onChange={setDivision}
                  options={DIVISIONS}
                />
              </span>
            ) : (
              <KnobCell
                id={`${instance.id}-rate`}
                label="Rate"
                value={rate}
                onChange={setRate}
                min={0.1}
                max={20}
                step={0.1}
                unit="Hz"
              />
            )}
          </LabelledControl>

          <LabelledControl label="Depth" htmlFor={`${instance.id}-depth`}>
            <KnobCell
              id={`${instance.id}-depth`}
              label="Depth"
              value={depth}
              onChange={setDepth}
              min={0}
              max={100}
              step={1}
              unit="%"
            />
          </LabelledControl>

          <LabelledControl label="Mix" htmlFor={`${instance.id}-mix`}>
            <KnobCell
              id={`${instance.id}-mix`}
              label="Mix"
              value={mix}
              onChange={setMix}
              min={0}
              max={100}
              step={1}
              unit="%"
            />
          </LabelledControl>
        </div>

        <div className="grid grid-cols-1 gap-x-6 gap-y-1 @md:grid-cols-2">
          <LabelledControl label="Shape" layout="row">
            <SegmentedControl aria-label="Shape" value={shape} onValueChange={setShape} options={SHAPES} />
          </LabelledControl>
          <LabelledControl label="Sync to transport" layout="row">
            <Switch label="Sync to transport" checked={sync} onCheckedChange={setSync} />
          </LabelledControl>
        </div>

        {/* The modulation shape over two cycles. Height follows depth. */}
        <div className="flex flex-col gap-2 border-t border-alpha/5 pt-3">
          <svg viewBox={`0 0 ${VIEW_W} ${VIEW_H}`} preserveAspectRatio="none" className="h-12 w-full" aria-hidden>
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
              d={lfoPath(shape, amp)}
              fill="none"
              stroke="var(--accent)"
              strokeWidth={1.5}
              strokeLinecap="round"
              strokeLinejoin="round"
              vectorEffect="non-scaling-stroke"
            />
          </svg>

          <div className="flex items-center gap-2">
            <span className="w-6 shrink-0 text-2xs font-semibold uppercase tracking-[0.08em] text-gray-950/40">
              in
            </span>
            <Meter label="Input level" orientation="horizontal" level={-14} peak={-11} thickness={6} scale />
            <span className="shrink-0 text-xs text-gray-950/60 tabular">−14.0 dB</span>
          </div>
          <div className="flex items-center gap-2">
            <span className="w-6 shrink-0 text-2xs font-semibold uppercase tracking-[0.08em] text-gray-950/40">
              out
            </span>
            <Meter label="Output level" orientation="horizontal" level={-16} peak={-12} thickness={6} scale />
            <span className="shrink-0 text-xs text-gray-950/60 tabular">−16.0 dB</span>
          </div>
        </div>
      </ToolViewBody>
    </ToolView>
  );
}

const PORTS: Port[] = [
  { id: "in", label: "in", kind: "audio", direction: "in" },
  { id: "rate", label: "rate", kind: "modulation", direction: "in" },
  { id: "out", label: "out", kind: "audio", direction: "out" },
];

export const tremoloExtension: ExtensionDef = {
  name: "tremolo",
  title: "Tremolo",
  description: "Amplitude modulation with a sine, triangle or square LFO that can follow the transport.",
  tools: [
    {
      id: "tremolo",
      name: "Tremolo",
      kind: "effect",
      accent: "pink",
      ports: PORTS,
      views: [{ id: "main", label: "Main", component: TremoloMain }],
    },
  ],
};
