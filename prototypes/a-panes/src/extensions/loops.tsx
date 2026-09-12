import * as React from "react";
import { Dices, Plus, RotateCcw, Volume2, VolumeX } from "lucide-react";
import {
  Badge,
  Button,
  Dot,
  IconButton,
  LabelledControl,
  NumericInput,
  Panel,
  SegmentedControl,
  Slider,
  Switch,
  ToolView,
  useInstanceState,
  cn,
} from "@/sdk";
import type { ExtensionDef, ToolViewProps } from "@/sdk";

/* ------------------------------------------------------------------ model */

interface Voice {
  id: string;
  name: string;
  target: string;
  steps: boolean[];
  length: number;
  rate: number;
  swing: number;
  mute: boolean;
}

const RULER_STEPS = 16;
const MIN_LEN = 2;
const MAX_LEN = 16;
const BAR_H = 34;
const STEP_MS = 250;

function pattern(length: number, hits: number[]): boolean[] {
  return Array.from({ length }, (_, i) => hits.includes(i));
}

const INITIAL_VOICES: Voice[] = [
  { id: "v1", name: "Voice 1", target: "Tone A", length: 8, steps: pattern(8, [0, 3, 6]), rate: 1, swing: 0, mute: false },
  { id: "v2", name: "Voice 2", target: "Tone B", length: 5, steps: pattern(5, [0, 2]), rate: 0.8, swing: 0, mute: false },
  { id: "v3", name: "Voice 3", target: "Tone C", length: 12, steps: pattern(12, [0, 4, 7]), rate: 1, swing: 0, mute: false },
];

function useVoices(instanceId: string) {
  const [voices, setVoices] = useInstanceState<Voice[]>(instanceId, "voices", INITIAL_VOICES);
  const patch = React.useCallback(
    (id: string, next: Partial<Voice>) => {
      setVoices((prev) => prev.map((v) => (v.id === id ? { ...v, ...next } : v)));
    },
    [setVoices],
  );
  const setLength = React.useCallback(
    (id: string, length: number) => {
      setVoices((prev) =>
        prev.map((v) =>
          v.id === id
            ? { ...v, length, steps: Array.from({ length }, (_, i) => v.steps[i] ?? false) }
            : v,
        ),
      );
    },
    [setVoices],
  );
  return { voices, setVoices, patch, setLength };
}

const gcd = (a: number, b: number): number => (b === 0 ? a : gcd(b, a % b));
const realignPeriod = (voices: Voice[]) =>
  voices.reduce((acc, v) => (acc * v.length) / gcd(acc, v.length), 1);

/** Elapsed ms since mount, coarse and cheap. Stops on unmount. */
function useElapsed(active: boolean) {
  const [ms, setMs] = React.useState(0);
  React.useEffect(() => {
    if (!active) return;
    const start = performance.now();
    const id = window.setInterval(() => setMs(performance.now() - start), 60);
    return () => window.clearInterval(id);
  }, [active]);
  return ms;
}

/** Track width, so the bars and the ruler share one pixel-per-step. */
function useTrackWidth() {
  const ref = React.useRef<HTMLDivElement>(null);
  const [width, setWidth] = React.useState(0);
  React.useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    setWidth(el.getBoundingClientRect().width);
    const ro = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) setWidth(entry.contentRect.width);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return { ref, width };
}

const INACTIVE = "color-mix(in oklab, var(--color-alpha) 6%, transparent)";
const CURRENT_OUTLINE = "color-mix(in oklab, var(--color-alpha) 55%, transparent)";

/* --------------------------------------------------------- patterns view */

function PatternsView({ instance }: ToolViewProps) {
  const { voices, setVoices, patch, setLength } = useVoices(instance.id);
  const [grid, setGrid] = useInstanceState<"1/8" | "1/16">(instance.id, "grid", "1/16");
  const [loop, setLoop] = useInstanceState(instance.id, "loop", true);
  const { ref, width } = useTrackWidth();
  const elapsed = useElapsed(loop);

  const toggle = (id: string, index: number) => {
    setVoices((prev) =>
      prev.map((v) =>
        v.id === id ? { ...v, steps: v.steps.map((s, i) => (i === index ? !s : s)) } : v,
      ),
    );
  };

  const clearAll = () =>
    setVoices((prev) => prev.map((v) => ({ ...v, steps: v.steps.map(() => false) })));
  const randomise = () =>
    setVoices((prev) => prev.map((v) => ({ ...v, steps: v.steps.map(() => Math.random() < 0.35) })));

  return (
    <ToolView
      title={instance.name}
      meta={`realigns every ${realignPeriod(voices)} steps`}
      actions={
        <>
          <IconButton label="Clear all" size="xs" variant="quiet" onClick={clearAll}>
            <RotateCcw />
          </IconButton>
          <IconButton label="Randomise" size="xs" variant="quiet" onClick={randomise}>
            <Dices />
          </IconButton>
        </>
      }
    >
      <div className="flex flex-col gap-5 p-4">
        <div className="flex items-center gap-3">
          <span className="text-xs text-gray-950/40">Grid</span>
          <SegmentedControl
            aria-label="Grid"
            size="xs"
            value={grid}
            onValueChange={setGrid}
            options={[
              { value: "1/8", label: "1/8" },
              { value: "1/16", label: "1/16" },
            ]}
          />
          <span className="ml-1 text-xs text-gray-950/40">Loop</span>
          <Switch size="xs" label="Loop" checked={loop} onCheckedChange={setLoop} />
          <Button size="xs" variant="subtle" className="ml-auto">
            <Plus />
            Add voice
          </Button>
        </div>

        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-3">
            <div className="w-28 shrink-0" />
            <div ref={ref} className="min-w-0 flex-1">
              <Ruler width={width} />
            </div>
            <div className="w-16 shrink-0" />
          </div>

          {voices.map((voice) => (
            <div key={voice.id} className="flex items-center gap-3">
              <div className="flex w-28 shrink-0 items-center gap-1">
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-medium text-gray-950">{voice.name}</div>
                  <div className="truncate text-xs text-gray-950/40">→ {voice.target}</div>
                </div>
                <IconButton
                  label={voice.mute ? `Unmute ${voice.name}` : `Mute ${voice.name}`}
                  size="xs"
                  variant="quiet"
                  active={voice.mute}
                  onClick={() => patch(voice.id, { mute: !voice.mute })}
                >
                  {voice.mute ? <VolumeX /> : <Volume2 />}
                </IconButton>
              </div>

              <div className="min-w-0 flex-1">
                <PatternBar
                  voice={voice}
                  width={width}
                  elapsed={loop ? elapsed : 0}
                  onToggle={(i) => toggle(voice.id, i)}
                  onLength={(l) => setLength(voice.id, l)}
                />
              </div>

              <div className="flex w-16 shrink-0 flex-col items-end gap-0.5">
                <Badge variant="accent" size="xs" tabular>
                  {voice.length} steps
                </Badge>
                <span className="text-2xs text-gray-950/40 tabular">{voice.rate.toFixed(2)}×</span>
              </div>
            </div>
          ))}
        </div>
      </div>
    </ToolView>
  );
}

function Ruler({ width }: { width: number }) {
  if (width <= 0) return <div className="h-3.5" />;
  const stepW = width / RULER_STEPS;
  return (
    <svg width={width} height={14} className="block text-gray-950/35">
      {[1, 5, 9, 13].map((n) => (
        <g key={n}>
          <text
            x={(n - 1) * stepW + 1}
            y={8}
            fill="currentColor"
            className="text-2xs tabular"
            style={{ fontSize: 10 }}
          >
            {n}
          </text>
          <line
            x1={(n - 1) * stepW + 0.5}
            x2={(n - 1) * stepW + 0.5}
            y1={10}
            y2={14}
            stroke="currentColor"
            strokeOpacity={0.5}
          />
        </g>
      ))}
    </svg>
  );
}

/** The one custom drawn area: a stretchable pattern bar on a shared 16-step ruler. */
function PatternBar({
  voice,
  width,
  elapsed,
  onToggle,
  onLength,
}: {
  voice: Voice;
  width: number;
  elapsed: number;
  onToggle: (index: number) => void;
  onLength: (length: number) => void;
}) {
  const drag = React.useRef<{ x: number; length: number } | null>(null);
  const [dragging, setDragging] = React.useState(false);

  if (width <= 0) return <div style={{ height: BAR_H }} />;

  const stepW = width / RULER_STEPS;
  const barW = stepW * voice.length;
  const pos = ((elapsed / STEP_MS) * voice.rate) % voice.length;
  const current = Math.floor(pos);

  const onHandleDown = (e: React.PointerEvent<SVGRectElement>) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { x: e.clientX, length: voice.length };
    setDragging(true);
  };
  const onHandleMove = (e: React.PointerEvent<SVGRectElement>) => {
    const start = drag.current;
    if (!start) return;
    const next = Math.round(start.length + (e.clientX - start.x) / stepW);
    onLength(Math.min(MAX_LEN, Math.max(MIN_LEN, next)));
  };
  const onHandleUp = () => {
    drag.current = null;
    setDragging(false);
  };

  return (
    <svg width={width} height={BAR_H} className="group block touch-none select-none">
      {/* the 16-step reference frame the bar is measured against */}
      <rect
        x={0.5}
        y={0.5}
        width={width - 1}
        height={BAR_H - 1}
        rx={5}
        fill="none"
        stroke="var(--color-alpha)"
        strokeOpacity={0.05}
      />
      {[4, 8, 12].map((n) =>
        n * stepW > barW ? (
          <line
            key={n}
            x1={n * stepW}
            x2={n * stepW}
            y1={4}
            y2={BAR_H - 4}
            stroke="var(--color-alpha)"
            strokeOpacity={0.06}
          />
        ) : null,
      )}

      <g opacity={voice.mute ? 0.35 : 1}>
        {voice.steps.map((on, i) => (
          <rect
            key={i}
            x={i * stepW + 1.5}
            y={2}
            width={Math.max(2, stepW - 3)}
            height={BAR_H - 4}
            rx={3}
            fill={on ? "var(--accent)" : INACTIVE}
            className="cursor-pointer transition-opacity duration-100 hover:opacity-70"
            onClick={() => onToggle(i)}
          />
        ))}
        {current >= 0 && current < voice.length && (
          <rect
            x={current * stepW + 1.5}
            y={2}
            width={Math.max(2, stepW - 3)}
            height={BAR_H - 4}
            rx={3}
            fill="none"
            stroke={CURRENT_OUTLINE}
            pointerEvents="none"
          />
        )}
        <line
          x1={pos * stepW}
          x2={pos * stepW}
          y1={1}
          y2={BAR_H - 1}
          stroke="var(--accent)"
          strokeOpacity={0.8}
          pointerEvents="none"
        />
      </g>

      {/* stretch handle on the right edge */}
      <rect
        x={Math.max(0, barW - 3)}
        y={2}
        width={3}
        height={BAR_H - 4}
        rx={1.5}
        fill="var(--accent)"
        className={cn(
          "transition-opacity duration-100",
          dragging ? "opacity-100" : "opacity-0 group-hover:opacity-60",
        )}
        pointerEvents="none"
      />
      <rect
        x={barW - 4}
        y={0}
        width={8}
        height={BAR_H}
        fill="transparent"
        style={{ cursor: "ew-resize" }}
        onPointerDown={onHandleDown}
        onPointerMove={onHandleMove}
        onPointerUp={onHandleUp}
        onPointerCancel={onHandleUp}
      >
        <title>Drag to stretch {voice.name}</title>
      </rect>
    </svg>
  );
}

/* ----------------------------------------------------------- voices view */

function VoicesView({ instance }: ToolViewProps) {
  const { voices, patch, setLength } = useVoices(instance.id);

  return (
    <ToolView title={instance.name} meta={`${voices.length} voices`}>
      <div className="@container flex flex-col gap-3 p-3">
        {voices.map((voice) => (
          <Panel key={voice.id} className="px-3 py-2.5">
            <div className="flex h-6 items-center gap-2">
              <Dot />
              <span className="text-sm font-medium text-gray-950">{voice.name}</span>
              <Badge variant="subtle" size="xs">
                {voice.target}
              </Badge>
              <span className="ml-auto text-2xs uppercase tracking-[0.08em] text-gray-950/40">
                Mute
              </span>
              <Switch
                size="xs"
                label={`Mute ${voice.name}`}
                checked={voice.mute}
                onCheckedChange={(mute) => patch(voice.id, { mute })}
              />
            </div>
            <div className="mt-2 grid grid-cols-1 gap-3 @[280px]:grid-cols-3 @[280px]:gap-4">
              <LabelledControl label="Length" htmlFor={`${voice.id}-length`}>
                <NumericInput
                  id={`${voice.id}-length`}
                  value={voice.length}
                  onChange={(length) => setLength(voice.id, length)}
                  min={MIN_LEN}
                  max={MAX_LEN}
                  step={1}
                  unit="steps"
                  size="xs"
                />
              </LabelledControl>
              <LabelledControl label="Rate" value={`${voice.rate.toFixed(2)}×`}>
                <Slider
                  label={`${voice.name} rate`}
                  value={voice.rate}
                  onChange={(rate) => patch(voice.id, { rate })}
                  min={0.25}
                  max={2}
                  step={0.05}
                  ticks={[1]}
                />
              </LabelledControl>
              <LabelledControl label="Swing" value={`${voice.swing.toFixed(0)} %`}>
                <Slider
                  label={`${voice.name} swing`}
                  value={voice.swing}
                  onChange={(swing) => patch(voice.id, { swing })}
                  min={0}
                  max={100}
                  step={1}
                />
              </LabelledControl>
            </div>
          </Panel>
        ))}
      </div>
    </ToolView>
  );
}

/* ------------------------------------------------------------- extension */

export const loopsExtension: ExtensionDef = {
  name: "rhythm-loops",
  title: "Rhythm Loops",
  description: "Independent looping patterns per voice; stretch a pattern by dragging its edge.",
  tools: [
    {
      id: "loops",
      name: "Loops",
      kind: "sequencer",
      accent: "yellow",
      ports: [
        { id: "clock", label: "Clock", kind: "event", direction: "in" },
        { id: "voice-1", label: "Voice 1", kind: "event", direction: "out" },
        { id: "voice-2", label: "Voice 2", kind: "event", direction: "out" },
        { id: "voice-3", label: "Voice 3", kind: "event", direction: "out" },
      ],
      views: [
        { id: "patterns", label: "Patterns", component: PatternsView },
        { id: "voices", label: "Voices", component: VoicesView },
      ],
    },
  ],
};
