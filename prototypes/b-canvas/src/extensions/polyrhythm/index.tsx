import * as React from "react";
import type { ToolDefinition, ToolViewProps } from "@/core/sdk";
import { Badge, Field, Knob, NumericInput, SegmentedControl, Slider, Switch, cn } from "@/ui";

/* ------------------------------------------------------------------ *
 * Shared fake data. One module-level store so both views of the same
 * instance show (and change) the same patterns.
 * ------------------------------------------------------------------ */

type VoiceId = "a" | "b" | "c";
type Grid = "1/8" | "1/16" | "free";

interface Voice {
  id: VoiceId;
  name: string;
  /** Number of steps in the pattern. */
  steps: number;
  /** Which steps fire. Length always equals `steps`. */
  pattern: boolean[];
  /** Seconds the whole pattern takes. Voices drift because these differ. */
  length: number;
  /** 0 to 1. */
  swing: number;
  muted: boolean;
}

interface PolyState {
  voices: Voice[];
  /** Base cycle in seconds, used for the grid and the ratio readout. */
  cycle: number;
  grid: Grid;
  /** 0 to 100 %. */
  humanise: number;
}

let state: PolyState = {
  cycle: 2,
  grid: "1/16",
  humanise: 18,
  voices: [
    { id: "a", name: "Voice A", steps: 4, pattern: [true, false, false, true], length: 2, swing: 0.12, muted: false },
    { id: "b", name: "Voice B", steps: 5, pattern: [true, false, true, false, true], length: 2.5, swing: 0, muted: false },
    { id: "c", name: "Voice C", steps: 3, pattern: [true, false, true], length: 2.4, swing: 0.22, muted: false },
  ],
};

const subscribers = new Set<() => void>();
const subscribe = (fn: () => void) => {
  subscribers.add(fn);
  return () => {
    subscribers.delete(fn);
  };
};
const getSnapshot = () => state;
function commit(next: PolyState) {
  state = next;
  for (const fn of subscribers) fn();
}
function usePoly(): PolyState {
  return React.useSyncExternalStore(subscribe, getSnapshot);
}

function patchVoice(id: VoiceId, patch: (v: Voice) => Voice) {
  commit({ ...state, voices: state.voices.map((v) => (v.id === id ? patch(v) : v)) });
}
const setLength = (id: VoiceId, length: number) => patchVoice(id, (v) => ({ ...v, length: clampLength(length) }));
const setSwing = (id: VoiceId, swing: number) => patchVoice(id, (v) => ({ ...v, swing }));
const setMuted = (id: VoiceId, muted: boolean) => patchVoice(id, (v) => ({ ...v, muted }));
const toggleStep = (id: VoiceId, index: number) =>
  patchVoice(id, (v) => ({ ...v, pattern: v.pattern.map((on, i) => (i === index ? !on : on)) }));
const setSteps = (id: VoiceId, steps: number) =>
  patchVoice(id, (v) => {
    const n = Math.max(1, Math.min(16, Math.round(steps)));
    const pattern = Array.from({ length: n }, (_, i) => v.pattern[i] ?? false);
    return { ...v, steps: n, pattern };
  });

/* ------------------------------------------------------------------ *
 * Geometry and maths
 * ------------------------------------------------------------------ */

const LABEL_W = 80;
const READ_W = 48;
const SVG_W = 256;
const HANDLE_ROOM = 14;
const PLOT_W = SVG_W - HANDLE_ROOM;
const TIME_MAX = 4;
const PPS = PLOT_W / TIME_MAX; // px per second
const LANE_H = 32;
const LANE_GAP = 6;
const LANES_H = LANE_H * 3 + LANE_GAP * 2;
const RULER_H = 22;
const SVG_H = LANES_H + RULER_H;
const PLAYHEAD = 1.35; // seconds, purely visual

const laneY = (i: number) => i * (LANE_H + LANE_GAP);
const clampLength = (s: number) => Math.min(TIME_MAX, Math.max(0.4, Math.round(s * 1000) / 1000));

/** Seconds per grid division, or 0 when the grid is free. */
function gridStep(cycle: number, grid: Grid) {
  if (grid === "free") return 0;
  return cycle / (grid === "1/8" ? 8 : 16);
}
function snapLength(seconds: number, cycle: number, grid: Grid) {
  const step = gridStep(cycle, grid);
  if (step <= 0) return clampLength(seconds);
  return clampLength(Math.round(seconds / step) * step);
}

/** How long until every unmuted voice lines up again. */
function combinedCycle(voices: Voice[]): number | null {
  const ms = voices.filter((v) => !v.muted).map((v) => Math.max(1, Math.round(v.length * 1000)));
  if (ms.length === 0) return null;
  const gcd = (a: number, b: number): number => (b === 0 ? a : gcd(b, a % b));
  let acc = ms[0] as number;
  for (const m of ms.slice(1)) {
    acc = (acc / gcd(acc, m)) * m;
    if (acc > 900_000) return null;
  }
  return acc / 1000;
}

/* ------------------------------------------------------------------ *
 * View 1: Patterns (the one custom SVG area)
 * ------------------------------------------------------------------ */

function PatternsView({ instanceName }: ToolViewProps) {
  const { voices, cycle, grid } = usePoly();
  const [dragging, setDragging] = React.useState<VoiceId | null>(null);
  const drag = React.useRef<{ id: VoiceId; x: number; start: number } | null>(null);
  const total = combinedCycle(voices);

  const startDrag = (e: React.PointerEvent<SVGRectElement>, v: Voice) => {
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { id: v.id, x: e.clientX, start: v.length };
    setDragging(v.id);
  };
  const moveDrag = (e: React.PointerEvent<SVGRectElement>) => {
    const d = drag.current;
    if (!d) return;
    setLength(d.id, snapLength(d.start + (e.clientX - d.x) / PPS, state.cycle, state.grid));
  };
  const endDrag = () => {
    drag.current = null;
    setDragging(null);
  };

  const ticks = Array.from({ length: TIME_MAX * 2 + 1 }, (_, i) => i / 2);

  return (
    <div className="flex flex-col gap-3">
      <div className="flex gap-2 rounded-lg border border-alpha/10 bg-gray-100 p-2">
        {/* lane labels */}
        <div className="relative shrink-0" style={{ width: LABEL_W, height: SVG_H }}>
          {voices.map((v, i) => (
            <div
              key={v.id}
              className="absolute right-0 left-0 flex items-center gap-1.5"
              style={{ top: laneY(i), height: LANE_H }}
            >
              <span className={cn("flex-1 truncate font-medium text-xs", v.muted ? "text-gray-700" : "text-gray-950")}>
                {v.name}
              </span>
              <Switch size="xs" checked={v.muted} onCheckedChange={(on) => setMuted(v.id, on)} label={`Mute ${v.name}`} />
            </div>
          ))}
        </div>

        {/* pattern lanes */}
        <svg
          width={SVG_W}
          height={SVG_H}
          viewBox={`0 0 ${SVG_W} ${SVG_H}`}
          role="group"
          aria-label={`${instanceName} pattern lanes`}
          className="shrink-0 touch-none select-none"
        >
          {voices.map((v, i) => {
            const y = laneY(i);
            const barW = v.length * PPS;
            const cell = barW / v.steps;
            return (
              <g key={v.id} opacity={v.muted ? 0.4 : 1}>
                <rect
                  x={0}
                  y={y}
                  width={barW}
                  height={LANE_H}
                  rx={8}
                  fill="var(--color-gray-300)"
                  stroke="var(--color-alpha)"
                  strokeOpacity={0.08}
                />
                {v.pattern.map((on, k) => {
                  const cx = k * cell;
                  return (
                    <g key={k}>
                      <rect
                        x={cx + 3}
                        y={y + 7}
                        width={Math.max(2, cell - 6)}
                        height={LANE_H - 14}
                        rx={4}
                        fill={on ? "var(--accent)" : "var(--color-gray-200)"}
                        fillOpacity={on ? 1 : 0.55}
                        stroke={on ? "none" : "var(--accent)"}
                        strokeOpacity={on ? 1 : 0.45}
                      />
                      <rect
                        x={cx}
                        y={y}
                        width={cell}
                        height={LANE_H}
                        rx={4}
                        role="button"
                        aria-label={`${v.name} step ${k + 1} ${on ? "on" : "off"}`}
                        onClick={() => toggleStep(v.id, k)}
                        className="cursor-pointer fill-transparent hover:fill-alpha/8"
                      />
                    </g>
                  );
                })}

                {/* stretch handle */}
                <g className="group/handle">
                  <rect
                    x={barW - 3.5}
                    y={y + 8}
                    width={7}
                    height={LANE_H - 16}
                    rx={3.5}
                    fill={dragging === v.id ? "var(--accent)" : "var(--color-gray-700)"}
                    className="group-hover/handle:fill-(--accent)"
                  />
                  <line
                    x1={barW - 1.25}
                    y1={y + 13}
                    x2={barW - 1.25}
                    y2={y + LANE_H - 13}
                    stroke="var(--color-gray-200)"
                    strokeWidth={1}
                  />
                  <line
                    x1={barW + 1.25}
                    y1={y + 13}
                    x2={barW + 1.25}
                    y2={y + LANE_H - 13}
                    stroke="var(--color-gray-200)"
                    strokeWidth={1}
                  />
                  <rect
                    x={barW - 9}
                    y={y}
                    width={18}
                    height={LANE_H}
                    rx={4}
                    tabIndex={0}
                    role="slider"
                    aria-label={`${v.name} cycle length`}
                    aria-valuemin={0.4}
                    aria-valuemax={TIME_MAX}
                    aria-valuenow={v.length}
                    aria-valuetext={`${v.length.toFixed(2)} seconds`}
                    onPointerDown={(e) => startDrag(e, v)}
                    onPointerMove={moveDrag}
                    onPointerUp={endDrag}
                    onPointerCancel={endDrag}
                    onKeyDown={(e) => {
                      const nudge = gridStep(cycle, grid) || 0.05;
                      if (e.key === "ArrowRight") setLength(v.id, v.length + nudge);
                      else if (e.key === "ArrowLeft") setLength(v.id, v.length - nudge);
                      else return;
                      e.preventDefault();
                    }}
                    className="focus-ring cursor-ew-resize fill-transparent"
                  />
                </g>
              </g>
            );
          })}

          {/* ruler */}
          {ticks.map((t) => {
            const whole = Number.isInteger(t);
            return (
              <line
                key={t}
                x1={t * PPS}
                y1={LANES_H + 6}
                x2={t * PPS}
                y2={LANES_H + (whole ? 12 : 9)}
                stroke="var(--color-gray-500)"
                strokeWidth={1}
              />
            );
          })}
          {[0, 1, 2, 3, 4].map((t) => (
            <text
              key={t}
              x={t * PPS}
              y={LANES_H + 22}
              fontSize={10}
              fill="var(--color-gray-700)"
              textAnchor={t === 0 ? "start" : t === TIME_MAX ? "end" : "middle"}
            >
              {t}s
            </text>
          ))}

          {/* playhead */}
          <g>
            <line
              x1={PLAYHEAD * PPS}
              y1={0}
              x2={PLAYHEAD * PPS}
              y2={LANES_H + 4}
              stroke="var(--color-gray-950)"
              strokeOpacity={0.65}
              strokeWidth={1}
            />
            <path
              d={`M ${PLAYHEAD * PPS - 3.5} ${LANES_H + 10} L ${PLAYHEAD * PPS + 3.5} ${LANES_H + 10} L ${PLAYHEAD * PPS} ${LANES_H + 4} Z`}
              fill="var(--color-gray-950)"
              fillOpacity={0.65}
            />
          </g>
        </svg>

        {/* per-lane readout */}
        <div className="relative shrink-0" style={{ width: READ_W, height: SVG_H }}>
          {voices.map((v, i) => (
            <div
              key={v.id}
              className="absolute right-0 left-0 flex flex-col justify-center"
              style={{ top: laneY(i), height: LANE_H }}
            >
              <span className={cn("tabular text-right text-xs", v.muted ? "text-gray-700" : "text-gray-950")}>
                {v.length.toFixed(2)} s
              </span>
              <span className="tabular text-right text-2xs text-gray-700">{(v.length / cycle).toFixed(2)}×</span>
            </div>
          ))}
        </div>
      </div>

      {/* transport row */}
      <div className="flex items-center gap-3">
        <Field label="Cycle" layout="row" className="w-[136px] shrink-0">
          <NumericInput
            value={cycle}
            onChange={(n) => commit({ ...state, cycle: Math.min(8, Math.max(0.5, n)) })}
            min={0.5}
            max={8}
            step={0.01}
            precision={2}
            unit="s"
            size="xs"
          />
        </Field>
        <SegmentedControl
          size="xs"
          label="Step grid"
          value={grid}
          onValueChange={(g) => commit({ ...state, grid: g })}
          options={[
            { value: "1/8", label: "1/8" },
            { value: "1/16", label: "1/16" },
            { value: "free", label: "free" },
          ]}
        />
        <span className="flex-1" />
        <Badge variant="accent-subtle" size="sm" className="tabular">
          {total === null ? "No voices playing" : `Repeats every ${total % 1 === 0 ? total.toFixed(0) : total.toFixed(1)} s`}
        </Badge>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ *
 * View 2: Voices (parameter list on the same data)
 * ------------------------------------------------------------------ */

const ROW = "grid grid-cols-[1fr_54px_64px_36px_32px] items-center gap-1.5";

function VoicesView(_props: ToolViewProps) {
  const { voices, humanise } = usePoly();
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-2">
        <div className={cn(ROW, "text-2xs text-gray-700")}>
          <span>Voice</span>
          <span className="pl-1.5">Steps</span>
          <span className="pl-1.5">Length</span>
          <span className="text-center">Swing</span>
          <span className="text-center">Mute</span>
        </div>
        {voices.map((v) => (
          <div key={v.id} className={ROW}>
            <span className={cn("truncate font-medium text-xs", v.muted ? "text-gray-700" : "text-gray-950")}>
              {v.name}
            </span>
            <NumericInput
              value={v.steps}
              onChange={(n) => setSteps(v.id, n)}
              min={1}
              max={16}
              step={0.06}
              size="xs"
              block
              label={`${v.name} steps`}
            />
            <NumericInput
              value={v.length}
              onChange={(n) => setLength(v.id, n)}
              min={0.4}
              max={TIME_MAX}
              step={0.02}
              precision={2}
              unit="s"
              size="xs"
              block
              label={`${v.name} length in seconds`}
            />
            <Knob
              value={v.swing}
              onChange={(n) => setSwing(v.id, n)}
              size={28}
              label={`${v.name} swing`}
              className="mx-auto"
            />
            <Switch
              size="xs"
              checked={v.muted}
              onCheckedChange={(on) => setMuted(v.id, on)}
              label={`Mute ${v.name}`}
              className="mx-auto"
            />
          </div>
        ))}
      </div>
      <Field label="Humanise" layout="row" value={`${Math.round(humanise)} %`}>
        <Slider
          value={humanise}
          onChange={(n) => commit({ ...state, humanise: n })}
          min={0}
          max={100}
          step={1}
          size="sm"
          aria-valuetext={`${Math.round(humanise)} percent`}
        />
      </Field>
    </div>
  );
}

export const tool: ToolDefinition = {
  type: "polyrhythm",
  name: "Polyrhythm",
  description: "Three voices with their own step pattern and cycle length, so they drift against each other.",
  accent: "peach",
  inputs: [
    { id: "reset", label: "Reset", kind: "event" },
    { id: "rate", label: "Rate", kind: "mod" },
  ],
  outputs: [
    { id: "voice-a", label: "Voice A", kind: "event" },
    { id: "voice-b", label: "Voice B", kind: "event" },
    { id: "voice-c", label: "Voice C", kind: "event" },
  ],
  views: [
    { id: "patterns", label: "Patterns", width: 440, component: PatternsView },
    { id: "voices", label: "Voices", width: 340, component: VoicesView },
  ],
};
