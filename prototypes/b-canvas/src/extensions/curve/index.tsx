import { Minus, Plus } from "lucide-react";
import * as React from "react";
import type { ToolDefinition, ToolViewProps } from "@/core/sdk";
import { Field, IconButton, NumericInput, SegmentedControl, Slider, Switch } from "@/ui";

/* ---------------------------------------------------------------- state --
   One module-level store so the Draw and Points views of an instance always
   show the same breakpoints. Fake data: a filter sweep over two bars. */

type Mode = "once" | "loop" | "pingpong";

interface Breakpoint {
  /** 0..1 along the curve length. */
  t: number;
  /** 0..1 modulation amount. */
  v: number;
}

interface CurveState {
  points: Breakpoint[];
  length: number;
  mode: Mode;
  sync: boolean;
  depth: number;
  smooth: number;
}

let state: CurveState = {
  points: [
    { t: 0, v: 0.16 },
    { t: 0.11, v: 0.94 },
    { t: 0.31, v: 0.58 },
    { t: 0.56, v: 0.79 },
    { t: 0.79, v: 0.27 },
    { t: 1, v: 0.05 },
  ],
  length: 2,
  mode: "loop",
  sync: true,
  depth: 72,
  smooth: 68,
};

const listeners = new Set<() => void>();
const subscribe = (fn: () => void) => {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
};
const getSnapshot = () => state;
const update = (patch: Partial<CurveState>) => {
  state = { ...state, ...patch };
  for (const fn of listeners) fn();
};
const useCurve = () => React.useSyncExternalStore(subscribe, getSnapshot);

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

function movePoint(index: number, next: Partial<Breakpoint>) {
  const points = state.points.map((p, i) => {
    if (i !== index) return p;
    const lo = index === 0 ? 0 : state.points[index - 1]!.t + 0.03;
    const hi = index === state.points.length - 1 ? 1 : state.points[index + 1]!.t - 0.03;
    const t = next.t === undefined ? p.t : clamp(next.t, lo, hi);
    const v = next.v === undefined ? p.v : clamp(next.v, 0, 1);
    return { t, v };
  });
  update({ points });
}

/* ----------------------------------------------------------------- maths --
   Catmull-Rom through the breakpoints, converted to cubic beziers. `smooth`
   is the tension: 0 gives straight segments, 1 a fully rounded curve. */

const W = 316; // card body width at view width 340
const H = 144;
const PAD_X = 14;
const PAD_Y = 14;
const PW = W - PAD_X * 2;
const PH = H - PAD_Y * 2;

interface XY {
  x: number;
  y: number;
}
interface Seg {
  a: XY;
  c1: XY;
  c2: XY;
  b: XY;
}

const toXY = (p: Breakpoint): XY => ({ x: PAD_X + p.t * PW, y: PAD_Y + (1 - p.v) * PH });

function buildSegments(points: Breakpoint[], k: number): Seg[] {
  const p = points.map(toXY);
  const segs: Seg[] = [];
  for (let i = 0; i < p.length - 1; i++) {
    const p0 = p[i - 1] ?? p[i]!;
    const a = p[i]!;
    const b = p[i + 1]!;
    const p3 = p[i + 2] ?? b;
    segs.push({
      a,
      b,
      c1: { x: clamp(a.x + ((b.x - p0.x) / 4) * k, a.x, b.x), y: a.y + ((b.y - p0.y) / 4) * k },
      c2: { x: clamp(b.x - ((p3.x - a.x) / 4) * k, a.x, b.x), y: b.y - ((p3.y - a.y) / 4) * k },
    });
  }
  return segs;
}

const pathOf = (segs: Seg[]) =>
  segs.reduce(
    (d, s) => `${d} C ${s.c1.x} ${s.c1.y}, ${s.c2.x} ${s.c2.y}, ${s.b.x} ${s.b.y}`,
    `M ${segs[0]?.a.x ?? PAD_X} ${segs[0]?.a.y ?? PAD_Y + PH}`,
  );

const cubic = (p0: number, p1: number, p2: number, p3: number, u: number) => {
  const m = 1 - u;
  return m * m * m * p0 + 3 * m * m * u * p1 + 3 * m * u * u * p2 + u * u * u * p3;
};

/** y of the curve at a given x, by bisecting the segment that spans it. */
function yAtX(segs: Seg[], x: number): number {
  const seg = segs.find((s) => x >= s.a.x && x <= s.b.x) ?? segs[segs.length - 1];
  if (!seg) return PAD_Y + PH;
  let lo = 0;
  let hi = 1;
  for (let i = 0; i < 24; i++) {
    const u = (lo + hi) / 2;
    if (cubic(seg.a.x, seg.c1.x, seg.c2.x, seg.b.x, u) < x) lo = u;
    else hi = u;
  }
  return cubic(seg.a.y, seg.c1.y, seg.c2.y, seg.b.y, (lo + hi) / 2);
}

const PLAYHEAD = 0.44;

/* ------------------------------------------------------------- draw view --*/

const modeOptions: { value: Mode; label: string }[] = [
  { value: "once", label: "Once" },
  { value: "loop", label: "Loop" },
  { value: "pingpong", label: "Ping-pong" },
];

function DrawView(_props: ToolViewProps) {
  const { points, length, mode, sync, depth, smooth } = useCurve();
  const svgRef = React.useRef<SVGSVGElement>(null);
  const [dragging, setDragging] = React.useState<number | null>(null);

  const segs = buildSegments(points, smooth / 100);
  const curve = pathOf(segs);
  const area = `${curve} L ${PAD_X + PW} ${PAD_Y + PH} L ${PAD_X} ${PAD_Y + PH} Z`;
  const headX = PAD_X + PLAYHEAD * PW;
  const headY = yAtX(segs, headX);

  const grid = [0, 1, 2, 3, 4];
  const ticks = grid.map((i) => (i * length) / 4);

  const onMove = (index: number) => (e: React.PointerEvent<SVGCircleElement>) => {
    if (dragging !== index) return;
    const rect = svgRef.current?.getBoundingClientRect();
    if (!rect) return;
    const scale = rect.width / W;
    const x = (e.clientX - rect.left) / scale;
    const y = (e.clientY - rect.top) / scale;
    movePoint(index, { t: clamp((x - PAD_X) / PW, 0, 1), v: clamp(1 - (y - PAD_Y) / PH, 0, 1) });
  };

  return (
    <div className="flex flex-col gap-3">
      <div>
        <div className="rounded-lg bg-gray-100">
          <svg
            ref={svgRef}
            viewBox={`0 0 ${W} ${H}`}
            className="block h-[144px] w-full touch-none"
            role="img"
            aria-label="Modulation curve, drag the breakpoints to reshape it"
          >
            <g stroke="var(--color-gray-400)" strokeOpacity={0.28} strokeWidth={1}>
              {grid.map((i) => (
                <line key={`v${i}`} x1={PAD_X + (i * PW) / 4} y1={PAD_Y} x2={PAD_X + (i * PW) / 4} y2={PAD_Y + PH} />
              ))}
              {grid.map((i) => (
                <line key={`h${i}`} x1={PAD_X} y1={PAD_Y + (i * PH) / 4} x2={PAD_X + PW} y2={PAD_Y + (i * PH) / 4} />
              ))}
            </g>

            <path d={area} fill="var(--accent)" fillOpacity={0.12} />
            <path d={curve} fill="none" stroke="var(--accent)" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" />

            <line
              x1={headX}
              y1={PAD_Y - 5}
              x2={headX}
              y2={PAD_Y + PH + 5}
              stroke="var(--color-gray-950)"
              strokeOpacity={0.45}
              strokeWidth={1}
            />
            <circle cx={headX} cy={headY} r={3.5} fill="var(--color-gray-950)" stroke="var(--color-gray-100)" strokeWidth={1.5} />

            {points.map((p, i) => {
              const { x, y } = toXY(p);
              return (
                <circle
                  key={i}
                  cx={x}
                  cy={y}
                  r={5.5}
                  fill="var(--accent)"
                  className="cursor-grab touch-none active:cursor-grabbing"
                  onPointerDown={(e) => {
                    e.currentTarget.setPointerCapture(e.pointerId);
                    setDragging(i);
                  }}
                  onPointerMove={onMove(i)}
                  onPointerUp={() => setDragging(null)}
                  onPointerCancel={() => setDragging(null)}
                />
              );
            })}
            {points.map((p, i) => {
              const { x, y } = toXY(p);
              return <circle key={i} cx={x} cy={y} r={2} fill="var(--color-gray-100)" pointerEvents="none" />;
            })}
          </svg>
        </div>
        <div className="mt-1 flex items-center justify-between px-[14px]">
          {ticks.map((t, i) => (
            <span key={i} className="tabular text-2xs text-gray-700">
              {i === 0 ? "0" : `${t.toFixed(1)} s`}
            </span>
          ))}
        </div>
      </div>

      <div className="flex items-center gap-3">
        <Field label="Length" layout="row" className="shrink-0">
          <NumericInput
            size="xs"
            value={length}
            onChange={(v) => update({ length: v })}
            min={0.25}
            max={16}
            step={0.05}
            precision={2}
            unit="s"
          />
        </Field>
        <span className="flex-1" />
        <SegmentedControl
          size="xs"
          label="Playback mode"
          value={mode}
          onValueChange={(v) => update({ mode: v })}
          options={modeOptions}
        />
        <Switch size="xs" accent checked={sync} onCheckedChange={(v) => update({ sync: v })} label="Sync to transport" />
      </div>

      <div className="flex flex-col gap-2">
        <Field label="Depth" layout="row" value={`${Math.round(depth)} %`}>
          <Slider size="sm" min={0} max={100} step={1} value={depth} onChange={(v) => update({ depth: v })} />
        </Field>
        <Field label="Smooth" layout="row" value={`${Math.round(smooth)} %`}>
          <Slider size="sm" min={0} max={100} step={1} value={smooth} onChange={(v) => update({ smooth: v })} />
        </Field>
      </div>
    </div>
  );
}

/* ----------------------------------------------------------- points view --*/

function addPoint() {
  const pts = state.points;
  if (pts.length >= 7) return;
  const i = pts.length - 2;
  const a = pts[i]!;
  const b = pts[i + 1]!;
  const next = [...pts];
  next.splice(i + 1, 0, { t: (a.t + b.t) / 2, v: (a.v + b.v) / 2 });
  update({ points: next });
}

function removePoint() {
  const pts = state.points;
  if (pts.length <= 3) return;
  update({ points: pts.filter((_, i) => i !== pts.length - 2) });
}

function PointsView(_props: ToolViewProps) {
  const { points, length } = useCurve();
  const cols = "grid grid-cols-[18px_1fr_1fr] items-center gap-x-2";

  return (
    <div className="flex flex-col gap-2">
      <div className={cols}>
        <span className="text-2xs text-gray-700">#</span>
        <span className="text-2xs text-gray-700">Time</span>
        <span className="text-2xs text-gray-700">Value</span>
      </div>

      <div className="flex flex-col gap-1">
        {points.map((p, i) => (
          <div key={i} className={cols}>
            <span className="tabular text-2xs text-gray-700">{i + 1}</span>
            <NumericInput
              size="xs"
              block
              label={`Point ${i + 1} time`}
              value={Number((p.t * length).toFixed(2))}
              onChange={(s) => movePoint(i, { t: s / length })}
              min={0}
              max={length}
              step={0.01}
              precision={2}
              unit="s"
            />
            <NumericInput
              size="xs"
              block
              label={`Point ${i + 1} value`}
              value={p.v}
              onChange={(v) => movePoint(i, { v })}
              min={0}
              max={1}
              step={0.005}
              precision={2}
            />
          </div>
        ))}
      </div>

      <div className="flex items-center justify-between">
        <span className="tabular text-2xs text-gray-700">{points.length} breakpoints</span>
        <div className="flex items-center gap-1">
          <IconButton label="Remove a breakpoint" size="xs" variant="ghost-muted" disabled={points.length <= 3} onClick={removePoint}>
            <Minus />
          </IconButton>
          <IconButton label="Add a breakpoint" size="xs" variant="ghost-muted" disabled={points.length >= 7} onClick={addPoint}>
            <Plus />
          </IconButton>
        </div>
      </div>
    </div>
  );
}

export const tool: ToolDefinition = {
  type: "curve",
  name: "Curve",
  description: "Draw a shape and play it back as a modulation signal into any parameter.",
  accent: "pink",
  inputs: [{ id: "trigger", label: "Trigger", kind: "event" }],
  outputs: [{ id: "out", label: "Curve", kind: "mod" }],
  views: [
    { id: "draw", label: "Draw", width: 340, component: DrawView },
    { id: "points", label: "Points", width: 280, component: PointsView },
  ],
};
