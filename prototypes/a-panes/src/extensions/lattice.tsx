import * as React from "react";
import { RotateCcw, X } from "lucide-react";
import {
  IconButton,
  NumericInput,
  Panel,
  SegmentedControl,
  ToolView,
  ToolViewBody,
  useInstanceState,
} from "@/sdk";
import type { ExtensionDef, Port, ToolViewProps } from "@/sdk";

/* ------------------------------------------------------------------ ratios */

const gcd = (a: number, b: number): number => (b === 0 ? a : gcd(b, a % b));

interface LatticeNode {
  /** Powers of 3 (fifths) and of 5 (major thirds). */
  a: number;
  b: number;
  key: string;
  num: number;
  den: number;
  ratio: number;
  cents: number;
}

/**
 * The ratio 3^a · 5^b, folded into one octave [1, 2) and reduced.
 * Plain integer maths: the biggest value here is 1024/675.
 */
function nodeAt(a: number, b: number): LatticeNode {
  let num = 1;
  let den = 1;
  if (a > 0) num *= 3 ** a;
  else den *= 3 ** -a;
  if (b > 0) num *= 5 ** b;
  else den *= 5 ** -b;
  while (num >= den * 2) den *= 2;
  while (num < den) num *= 2;
  const g = gcd(num, den);
  num /= g;
  den /= g;
  const ratio = num / den;
  return { a, b, key: `${a},${b}`, num, den, ratio, cents: 1200 * Math.log2(ratio) };
}

const COLS = [-3, -2, -1, 0, 1, 2, 3];
const ROWS = [2, 1, 0, -1, -2]; // top row is the highest power of 5
const NODES: LatticeNode[] = ROWS.flatMap((b) => COLS.map((a) => nodeAt(a, b)));

function nodeByKey(key: string): LatticeNode | undefined {
  return NODES.find((n) => n.key === key);
}

/* ------------------------------------------------------------- shared state */

/** A just major scale: 1/1, 9/8, 5/4, 3/2, 5/3, 15/8. */
const DEFAULT_SELECTED = ["0,0", "2,0", "0,1", "1,0", "-1,1", "1,1"];
const DEFAULT_ROOT = 220;

/** Both views read and write the same instance state, so they always agree. */
function useLattice(instanceId: string) {
  const [selected, setSelected] = useInstanceState<string[]>(instanceId, "selected", DEFAULT_SELECTED);
  const [root, setRoot] = useInstanceState(instanceId, "root", DEFAULT_ROOT);

  const toggle = React.useCallback(
    (key: string) =>
      setSelected((prev) => (prev.includes(key) ? prev.filter((k) => k !== key) : [...prev, key])),
    [setSelected],
  );
  const reset = React.useCallback(() => {
    setSelected(DEFAULT_SELECTED);
    setRoot(DEFAULT_ROOT);
  }, [setSelected, setRoot]);

  return { selected, setSelected, root, setRoot, toggle, reset };
}

function ResetAction({ onReset }: { onReset: () => void }) {
  return (
    <IconButton size="xs" variant="quiet" label="Reset scale and root" onClick={onReset}>
      <RotateCcw />
    </IconButton>
  );
}

/* ------------------------------------------------------------ lattice view */

const CELL_W = 92;
const CELL_H = 58;
const NODE_W = 78;
const NODE_H = 44;
const VB_W = COLS.length * CELL_W;
const VB_H = ROWS.length * CELL_H;

const cx = (a: number) => (a + 3) * CELL_W + CELL_W / 2;
const cy = (b: number) => (2 - b) * CELL_H + CELL_H / 2;

/** Shrink the ratio text so long ones like 1024/675 still fit the node. */
function ratioFontSize(label: string) {
  return Math.min(17, (NODE_W * 0.92) / (label.length * 0.58));
}

function LatticeView({ instance }: ToolViewProps) {
  const { selected, root, setRoot, toggle, reset } = useLattice(instance.id);
  const [level, setLevel] = useInstanceState(instance.id, "level", -8);
  const [limit, setLimit] = useInstanceState<"5" | "7">(instance.id, "limit", "5");
  const [focused, setFocused] = React.useState<string | null>(null);

  return (
    <ToolView
      title={instance.name}
      meta={`${root.toFixed(1)} Hz · ${selected.length} notes`}
      actions={<ResetAction onReset={reset} />}
    >
      <ToolViewBody className="gap-3">
        {/* One dense row: root, level and limit. Number boxes drag vertically and accept typing. */}
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <div className="flex items-center gap-2">
            <label htmlFor={`${instance.id}-root`} className="text-xs font-medium text-gray-950/70">
              Root
            </label>
            <NumericInput
              id={`${instance.id}-root`}
              label="Root frequency"
              size="xs"
              className="w-24"
              value={root}
              onChange={setRoot}
              min={55}
              max={880}
              step={0.1}
              unit="Hz"
            />
          </div>
          <div className="flex items-center gap-2">
            <label htmlFor={`${instance.id}-level`} className="text-xs font-medium text-gray-950/70">
              Level
            </label>
            <NumericInput
              id={`${instance.id}-level`}
              label="Level"
              size="xs"
              className="w-20"
              value={level}
              onChange={setLevel}
              min={-60}
              max={0}
              step={0.5}
              unit="dB"
            />
          </div>
          <div className="ml-auto flex items-center gap-2">
            <span className="text-xs text-gray-950/40">Limit</span>
            <SegmentedControl
              aria-label="Prime limit"
              size="xs"
              value={limit}
              onValueChange={setLimit}
              options={[
                { value: "5", label: "5" },
                { value: "7", label: "7" },
              ]}
            />
          </div>
        </div>
        <svg
          viewBox={`0 0 ${VB_W} ${VB_H}`}
          preserveAspectRatio="xMidYMid meet"
          className="block w-full max-h-80"
          style={{ fontFamily: "inherit" }}
          role="group"
          aria-label="Frequency ratio lattice"
        >
          {/* connections between neighbouring nodes, drawn under the nodes */}
          <g stroke="var(--color-alpha)" strokeOpacity={0.08} strokeWidth={1}>
            {ROWS.map((b) =>
              COLS.slice(0, -1).map((a) => (
                <line
                  key={`h${a},${b}`}
                  x1={cx(a) + NODE_W / 2}
                  x2={cx(a + 1) - NODE_W / 2}
                  y1={cy(b)}
                  y2={cy(b)}
                />
              )),
            )}
            {ROWS.slice(0, -1).map((b) =>
              COLS.map((a) => (
                <line
                  key={`v${a},${b}`}
                  x1={cx(a)}
                  x2={cx(a)}
                  y1={cy(b) + NODE_H / 2}
                  y2={cy(b - 1) - NODE_H / 2}
                />
              )),
            )}
          </g>

          {NODES.map((node) => {
            const on = selected.includes(node.key);
            const isRoot = node.a === 0 && node.b === 0;
            const label = `${node.num}/${node.den}`;
            const cents = Math.round(node.cents);
            const x = cx(node.a) - NODE_W / 2;
            const y = cy(node.b) - NODE_H / 2;
            return (
              <g
                key={node.key}
                role="button"
                tabIndex={0}
                aria-pressed={on}
                aria-label={`${label}, ${cents} cents`}
                className={on ? "text-gray-200 outline-none" : "text-gray-950/70 outline-none"}
                onClick={() => toggle(node.key)}
                onKeyDown={(e) => {
                  if (e.key !== "Enter" && e.key !== " ") return;
                  e.preventDefault();
                  toggle(node.key);
                }}
                onFocus={() => setFocused(node.key)}
                onBlur={() => setFocused(null)}
              >
                <rect
                  x={x}
                  y={y}
                  width={NODE_W}
                  height={NODE_H}
                  rx={8}
                  fill={on ? "var(--accent)" : "color-mix(in oklab, var(--color-alpha) 5%, transparent)"}
                  className="transition-opacity duration-100 hover:opacity-80"
                />
                {isRoot && (
                  <rect
                    x={x - 3}
                    y={y - 3}
                    width={NODE_W + 6}
                    height={NODE_H + 6}
                    rx={11}
                    fill="none"
                    stroke="var(--accent)"
                    strokeOpacity={0.5}
                  />
                )}
                {focused === node.key && (
                  <rect
                    x={x - 2}
                    y={y - 2}
                    width={NODE_W + 4}
                    height={NODE_H + 4}
                    rx={10}
                    fill="none"
                    stroke="var(--color-lavender-500)"
                    strokeWidth={2}
                  />
                )}
                <text
                  x={cx(node.a)}
                  y={cy(node.b) - 2}
                  textAnchor="middle"
                  fill="currentColor"
                  fontWeight={500}
                  fontSize={ratioFontSize(label)}
                >
                  {label}
                </text>
                <text
                  x={cx(node.a)}
                  y={cy(node.b) + 14}
                  textAnchor="middle"
                  fill="currentColor"
                  fillOpacity={0.6}
                  fontSize={14}
                  className="tabular"
                >
                  {cents}
                </text>
              </g>
            );
          })}
        </svg>

      </ToolViewBody>
    </ToolView>
  );
}

/* -------------------------------------------------------------- scale view */

function ScaleView({ instance }: ToolViewProps) {
  const { selected, setSelected, root, reset } = useLattice(instance.id);
  const [limit] = useInstanceState<"5" | "7">(instance.id, "limit", "5");

  const rows = selected
    .map(nodeByKey)
    .filter((n): n is LatticeNode => n !== undefined)
    .sort((x, y) => x.ratio - y.ratio);

  return (
    <ToolView
      title={instance.name}
      meta={`${rows.length} notes · ${limit}-limit`}
      actions={<ResetAction onReset={reset} />}
    >
      <div className="flex flex-col gap-3 p-3">
        <Panel className="px-2 py-1">
          <div className="flex h-6 items-center gap-2 text-2xs font-semibold uppercase tracking-[0.08em] text-gray-950/40">
            <span className="w-16 shrink-0">Ratio</span>
            <span className="w-12 shrink-0 text-right">Cents</span>
            <span className="flex-1 text-right">Frequency</span>
            <span className="w-6 shrink-0" />
          </div>
          {rows.map((node) => (
            <div
              key={node.key}
              className="flex h-7 items-center gap-2 border-t border-alpha/5 text-sm"
            >
              <span className="w-16 shrink-0 truncate font-medium text-gray-950 tabular">
                {node.num}/{node.den}
              </span>
              <span className="w-12 shrink-0 text-right text-xs text-gray-950/50 tabular">
                {Math.round(node.cents)}
              </span>
              <span className="flex-1 text-right text-xs text-gray-950/70 tabular">
                {(root * node.ratio).toFixed(1)} Hz
              </span>
              <IconButton
                size="xs"
                variant="quiet"
                label={`Remove ${node.num}/${node.den}`}
                onClick={() => setSelected(selected.filter((k) => k !== node.key))}
              >
                <X />
              </IconButton>
            </div>
          ))}
          {rows.length === 0 && (
            <div className="flex h-7 items-center border-t border-alpha/5 text-xs text-gray-950/40">
              No notes selected. Pick nodes in the Lattice view.
            </div>
          )}
        </Panel>
        <div className="text-xs text-gray-950/50 tabular">Root {root.toFixed(1)} Hz</div>
      </div>
    </ToolView>
  );
}

/* ------------------------------------------------------------- extension */

const PORTS: Port[] = [
  { id: "trigger", label: "trigger", kind: "event", direction: "in" },
  { id: "pitch", label: "pitch", kind: "modulation", direction: "out" },
  { id: "gate", label: "gate", kind: "event", direction: "out" },
];

export const latticeExtension: ExtensionDef = {
  name: "ratio-lattice",
  title: "Ratio Lattice",
  description: "A just-intonation pitch lattice: pick notes as frequency ratios of fifths and thirds.",
  tools: [
    {
      id: "lattice",
      name: "Lattice",
      kind: "instrument",
      accent: "blue",
      ports: PORTS,
      views: [
        { id: "main", label: "Lattice", component: LatticeView },
        { id: "scale", label: "Scale", component: ScaleView },
      ],
    },
  ],
};
