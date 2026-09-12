import { ArrowLeft, Bell, Copy, Dice5, Hammer, Layers2, Play, Plus, Settings2, Trash2 } from "lucide-react";
import * as React from "react";
import { ACCENT_VAR, type Accent } from "@/sdk/types";
import {
  Badge,
  Button,
  ControlGroup,
  Dot,
  IconButton,
  Kbd,
  KbdShortcut,
  Knob,
  LabelledControl,
  Meter,
  NumericInput,
  Panel,
  SegmentedControl,
  Select,
  Separator,
  Slider,
  Switch,
  Tabs,
  ToolView,
  ToolViewBody,
  Tooltip,
  cn,
} from "@/ui";

const SWATCHES: { name: string; token: string; role: string }[] = [
  { name: "crust", token: "--color-gray-50", role: "window background" },
  { name: "mantle", token: "--color-gray-100", role: "panels, bars, tab strips" },
  { name: "base", token: "--color-gray-200", role: "tool view surface" },
  { name: "surface0", token: "--color-gray-300", role: "raised: popups, tooltips" },
  { name: "surface1", token: "--color-gray-400", role: "scrollbars" },
  { name: "surface2", token: "--color-gray-500", role: "" },
  { name: "overlay0", token: "--color-gray-600", role: "" },
  { name: "overlay1", token: "--color-gray-700", role: "" },
  { name: "overlay2", token: "--color-gray-800", role: "audio port colour" },
  { name: "subtext0", token: "--color-gray-900", role: "" },
  { name: "text", token: "--color-gray-950", role: "text, primary button fill" },
];

const ACCENTS: { name: string; token: string; role: string }[] = [
  { name: "lavender", token: "--color-lavender-500", role: "agent, focus ring" },
  { name: "green", token: "--color-green-500", role: "transport, ok" },
  { name: "peach", token: "--color-orange-500", role: "warning, event ports" },
  { name: "red", token: "--color-red-500", role: "error" },
  { name: "teal", token: "--color-teal-500", role: "Tone" },
  { name: "pink", token: "--color-pink-500", role: "Tremolo" },
  { name: "yellow", token: "--color-yellow-500", role: "Loops" },
  { name: "blue", token: "--color-blue-500", role: "Lattice, modulation ports" },
  { name: "flamingo", token: "--color-flamingo-500", role: "Mix" },
  { name: "mauve", token: "--color-purple-500", role: "" },
  { name: "sky", token: "--color-cyan-500", role: "" },
  { name: "sapphire", token: "--color-blue-600", role: "" },
  { name: "maroon", token: "--color-maroon-500", role: "" },
  { name: "rosewater", token: "--color-rosewater-500", role: "" },
];

const BUTTON_VARIANTS = ["primary", "subtle", "outline", "ghost", "quiet", "agent", "agent-subtle", "agent-ghost", "green", "green-subtle", "orange-subtle", "red", "red-subtle", "accent", "accent-subtle", "accent-ghost"] as const;
const BADGE_VARIANTS = ["primary", "subtle", "outline", "ghost", "agent", "green", "orange", "red", "blue", "accent"] as const;

export function Gallery() {
  return (
    <div className="min-h-screen bg-gray-50 text-gray-950" style={{ "--accent": ACCENT_VAR.teal } as React.CSSProperties}>
      <div className="sticky top-0 z-10 flex h-11 items-center gap-3 border-b border-alpha/5 bg-gray-50/90 px-6 backdrop-blur">
        <a href="#/" className="inline-flex h-7 items-center gap-1 rounded-md px-2 text-sm text-gray-950/60 hover:bg-alpha/5 hover:text-gray-950">
          <ArrowLeft className="size-4" /> Window
        </a>
        <Separator orientation="vertical" className="h-4" />
        <h1 className="text-sm font-medium">Primitives</h1>
        <span className="text-xs text-gray-950/40">Hooman Studio system, Catppuccin Mocha, dark only. Tool accent on this page: teal.</span>
      </div>

      <div className="mx-auto flex max-w-6xl flex-col gap-12 px-6 py-8">
        <Section title="Colour" note="Neutral scale keeps Hooman names: gray-50 is the darkest surface, gray-950 is text.">
          <div className="grid grid-cols-11 gap-1">
            {SWATCHES.map((s) => (
              <div key={s.name} className="flex flex-col gap-1.5">
                <div className="h-12 rounded-md border border-alpha/10" style={{ background: `var(${s.token})` }} />
                <div className="text-xs font-medium">{s.name}</div>
                <div className="text-2xs leading-3 text-gray-950/40">{s.role}</div>
              </div>
            ))}
          </div>
          <div className="mt-4 grid grid-cols-7 gap-1 lg:grid-cols-14">
            {ACCENTS.map((s) => (
              <div key={s.name} className="flex flex-col gap-1.5">
                <div className="h-8 rounded-md" style={{ background: `var(${s.token})` }} />
                <div className="text-xs font-medium">{s.name}</div>
                <div className="text-2xs leading-3 text-gray-950/40">{s.role}</div>
              </div>
            ))}
          </div>
        </Section>

        <Section title="Type" note="InterDisplay with ss03 and cv01. Numbers always tabular. Mono for paths and build output.">
          <div className="grid grid-cols-2 gap-6">
            <div className="flex flex-col gap-3">
              <div className="text-xl font-medium">Three voices, realigning every 120 steps</div>
              <div className="text-lg font-medium">Give these three voices independent rhythms</div>
              <div className="text-base">Body text, 16px. Rarely used in the dense chrome.</div>
              <div className="text-sm">Default text, 14px. Labels are font-medium, values tabular.</div>
              <div className="text-xs text-gray-950/60">Meta text, 12px, at 60% opacity.</div>
              <div className="text-2xs font-semibold uppercase tracking-[0.08em] text-gray-950/40">Group heading, 10px</div>
            </div>
            <div className="flex flex-col gap-3">
              <div className="flex gap-6 text-sm">
                <span className="tabular">1:24.350 · 220.0 Hz · −12.0 dB (tabular)</span>
                <span className="text-gray-950/50">1:24.350 · 220.0 Hz · −12.0 dB (proportional)</span>
              </div>
              <div className="font-mono text-xs text-gray-950/80">extensions/rhythm-loops/src/lib.rs</div>
              <pre className="rounded-md bg-gray-50 p-3 font-mono text-2xs leading-4 text-gray-950/70 ring-1 ring-alpha/5">
                {"   Compiling rhythm-loops v0.1.0\n    Finished `dev` profile in 1.34s\n  Runtime ready in 2.21s"}
              </pre>
            </div>
          </div>
        </Section>

        <Section title="Button" note="Sizes 24 / 28 / 32 / 40. Columns show default, hover, focus, disabled.">
          <StateGrid>
            {BUTTON_VARIANTS.map((v) => (
              <StateRow key={v} label={v}>
                {(state) => (
                  <Button variant={v} size="md" disabled={state === "disabled"}>
                    <Play /> Play
                  </Button>
                )}
              </StateRow>
            ))}
          </StateGrid>
          <Row label="Sizes">
            <Button variant="subtle" size="xs"><Plus />Add voice</Button>
            <Button variant="subtle" size="sm"><Plus />Add voice</Button>
            <Button variant="subtle" size="md"><Plus />Add voice</Button>
            <Button variant="subtle" size="lg"><Plus />Add voice</Button>
          </Row>
          <Row label="Icon buttons">
            <IconButton label="Settings" size="xs" variant="quiet"><Settings2 /></IconButton>
            <IconButton label="Settings" size="sm" variant="ghost"><Settings2 /></IconButton>
            <IconButton label="Settings" size="md" variant="subtle"><Settings2 /></IconButton>
            <IconButton label="Settings" size="lg" variant="outline"><Settings2 /></IconButton>
            <IconButton label="Delete" size="md" variant="red-subtle"><Trash2 /></IconButton>
            <IconButton label="Randomise" size="md" variant="accent-subtle"><Dice5 /></IconButton>
            <IconButton label="Active toggle" size="md" variant="ghost" active><Bell /></IconButton>
          </Row>
        </Section>

        <Section title="Badge and dot" note="Status and meta. xs 16px, sm 20px, md 24px, lg 28px.">
          <Row label="Variants">
            {BADGE_VARIANTS.map((v) => (
              <Badge key={v} variant={v} size="sm">{v}</Badge>
            ))}
          </Row>
          <Row label="Sizes and icons">
            <Badge size="xs" variant="accent" icon={<Layers2 />}>2 views</Badge>
            <Badge size="sm" variant="green" icon={<Hammer />} tabular>built 2.2 s</Badge>
            <Badge size="md" variant="agent">Building rhythm-loops…</Badge>
            <Badge size="lg" variant="red">Build failed · running previous build</Badge>
          </Row>
          <Row label="Dots">
            {(Object.keys(ACCENT_VAR) as Accent[]).map((a) => (
              <span key={a} className="inline-flex items-center gap-1.5 text-xs text-gray-950/70"><Dot color={ACCENT_VAR[a]} />{a}</span>
            ))}
            <span className="inline-flex items-center gap-1.5 text-xs text-gray-950/70"><Dot color="var(--color-lavender-500)" pulse />working</span>
          </Row>
        </Section>

        <Section title="Kbd, tooltip, separator">
          <Row label="Kbd">
            <Kbd>⌘</Kbd><Kbd>K</Kbd><KbdShortcut shortcut="mod+enter" /><KbdShortcut shortcut="shift+space" /><KbdShortcut shortcut="esc" />
          </Row>
          <Row label="Tooltip (hover or focus)">
            <Tooltip content="Play from 0:00"><Button variant="subtle" size="sm"><Play />Hover me</Button></Tooltip>
            <Tooltip content={<><span>Copy path</span><Kbd>⌘C</Kbd></>} side="bottom"><IconButton label="Copy" size="sm"><Copy /></IconButton></Tooltip>
          </Row>
          <Row label="Separator">
            <span className="text-sm">A</span><Separator orientation="vertical" className="h-4" /><span className="text-sm">B</span>
            <div className="w-40"><Separator /></div>
          </Row>
        </Section>

        <Section title="Segmented control, tabs, switch, select">
          <SegmentedDemo />
        </Section>

        <Section title="Labelled control, numeric input, slider" note="Drag a number box vertically to change it, click to type, arrow keys nudge, shift for fine.">
          <SliderDemo />
        </Section>

        <Section title="Knob and meter">
          <KnobDemo />
        </Section>

        <Section title="Tool view frame" note="The frame every extension view lives in. The pane sets --accent; header carries name, meta and up to two actions. Panels group controls inside.">
          <div className="h-72 overflow-hidden rounded-lg ring-1 ring-alpha/10" style={{ "--accent": ACCENT_VAR.pink } as React.CSSProperties}>
            <ToolView title="Tremolo" meta="main" actions={<IconButton label="Reset" size="xs" variant="quiet"><Settings2 /></IconButton>}>
              <ToolViewBody>
                <ControlGroup title="Modulation" action={<Badge size="xs" variant="accent">synced</Badge>}>
                  <div className="grid grid-cols-3 gap-4">
                    <Panel className="p-3 text-xs text-gray-950/60">Panel</Panel>
                    <Panel raised className="p-3 text-xs text-gray-950/60">Panel raised</Panel>
                    <Panel className="p-3 text-xs text-gray-950/60">Panel</Panel>
                  </div>
                </ControlGroup>
              </ToolViewBody>
            </ToolView>
          </div>
        </Section>
      </div>
    </div>
  );
}

/* Demos with state */

function SegmentedDemo() {
  const [grid, setGrid] = React.useState<"8" | "16">("16");
  const [tab, setTab] = React.useState<"patterns" | "voices" | "settings">("patterns");
  const [on, setOn] = React.useState(true);
  const [wave, setWave] = React.useState("sine");
  return (
    <>
      <Row label="Segmented">
        <SegmentedControl size="xs" value={grid} onValueChange={setGrid} options={[{ value: "8", label: "1/8" }, { value: "16", label: "1/16" }]} aria-label="Grid xs" />
        <SegmentedControl size="sm" value={grid} onValueChange={setGrid} options={[{ value: "8", label: "1/8" }, { value: "16", label: "1/16" }]} aria-label="Grid sm" />
        <SegmentedControl size="md" value={grid} onValueChange={setGrid} options={[{ value: "8", label: "1/8" }, { value: "16", label: "1/16" }]} aria-label="Grid md" />
      </Row>
      <Row label="Tabs">
        <Tabs value={tab} onValueChange={setTab} tabs={[{ value: "patterns", label: "Patterns" }, { value: "voices", label: "Voices" }, { value: "settings", label: "Settings" }]} />
      </Row>
      <Row label="Switch">
        <Switch label="Loop" checked={on} onCheckedChange={setOn} />
        <Switch label="Loop small" size="xs" checked={on} onCheckedChange={setOn} />
        <Switch label="Loop off" checked={false} onCheckedChange={() => {}} />
        <Switch label="Loop disabled" checked disabled onCheckedChange={() => {}} />
      </Row>
      <Row label="Select">
        <Select label="Wave xs" size="xs" value={wave} onChange={setWave} options={[{ value: "sine", label: "Sine" }, { value: "tri", label: "Triangle" }, { value: "saw", label: "Saw" }]} />
        <Select label="Wave sm" size="sm" value={wave} onChange={setWave} options={[{ value: "sine", label: "Sine" }, { value: "tri", label: "Triangle" }, { value: "saw", label: "Saw" }]} />
        <Select label="Wave md" size="md" value={wave} onChange={setWave} options={[{ value: "sine", label: "Sine" }, { value: "tri", label: "Triangle" }, { value: "saw", label: "Saw" }]} />
        <Select label="Wave disabled" size="sm" disabled value={wave} onChange={setWave} options={[{ value: "sine", label: "Sine" }]} />
      </Row>
    </>
  );
}

function SliderDemo() {
  const [freq, setFreq] = React.useState(220);
  const [level, setLevel] = React.useState(-12);
  const [pan, setPan] = React.useState(0.2);
  const [v, setV] = React.useState(0.65);
  return (
    <div className="grid grid-cols-[1fr_1fr_auto] gap-8">
      <div className="flex flex-col gap-4">
        <LabelledControl label="Frequency" hint="Hz" value={`${freq.toFixed(1)} Hz`} htmlFor="g-freq">
          <Slider id="g-freq" label="Frequency" value={freq} onChange={setFreq} min={20} max={2000} step={0.5} />
        </LabelledControl>
        <LabelledControl label="Level" value={`${level.toFixed(1)} dB`} htmlFor="g-level">
          <Slider id="g-level" label="Level" value={level} onChange={setLevel} min={-60} max={0} step={0.5} ticks={[-12, -6]} />
        </LabelledControl>
        <LabelledControl label="Pan" hint="bipolar" value={pan === 0 ? "C" : pan < 0 ? `L ${Math.round(-pan * 100)}` : `R ${Math.round(pan * 100)}`} htmlFor="g-pan">
          <Slider id="g-pan" label="Pan" value={pan} onChange={setPan} min={-1} max={1} step={0.01} origin={0} ticks={[0]} />
        </LabelledControl>
        <LabelledControl label="Disabled" value="0.65" htmlFor="g-dis">
          <Slider id="g-dis" label="Disabled" value={v} onChange={setV} min={0} max={1} disabled />
        </LabelledControl>
      </div>
      <div className="flex flex-col gap-3">
        <LabelledControl layout="row" label="Frequency" hint="20–2000">
          <NumericInput label="Frequency" value={freq} onChange={setFreq} min={20} max={2000} step={0.5} unit="Hz" className="w-28" />
        </LabelledControl>
        <LabelledControl layout="row" label="Level">
          <NumericInput label="Level" value={level} onChange={setLevel} min={-60} max={0} step={0.5} unit="dB" className="w-28" />
        </LabelledControl>
        <LabelledControl layout="row" label="Length" hint="steps">
          <NumericInput label="Length" value={8} onChange={() => {}} min={2} max={16} step={1} size="xs" className="w-20" />
        </LabelledControl>
        <LabelledControl layout="row" label="Rate">
          <NumericInput label="Rate" value={0.8} onChange={() => {}} min={0.25} max={2} step={0.05} format={(x) => `${x.toFixed(2)}×`} size="md" className="w-24" />
        </LabelledControl>
        <LabelledControl layout="row" label="Disabled">
          <NumericInput label="Disabled" value={12} onChange={() => {}} min={0} max={100} disabled className="w-28" />
        </LabelledControl>
        <LabelledControl layout="row" label="Switch in a row">
          <Switch label="Sync" checked onCheckedChange={() => {}} size="xs" />
        </LabelledControl>
      </div>
      <div className="flex h-56 items-stretch gap-4">
        <div className="flex flex-col items-center gap-2">
          <Slider label="Fader" orientation="vertical" value={level} onChange={setLevel} min={-60} max={6} step={0.5} thickness={4} ticks={[0, -6, -12]} />
          <span className="text-2xs text-gray-950/50 tabular">{level.toFixed(1)}</span>
        </div>
        <div className="flex flex-col items-center gap-2">
          <Slider label="Fader bipolar" orientation="vertical" value={pan} onChange={setPan} min={-1} max={1} origin={0} />
          <span className="text-2xs text-gray-950/50">pan</span>
        </div>
      </div>
    </div>
  );
}

function KnobDemo() {
  const [rate, setRate] = React.useState(4.5);
  const [depth, setDepth] = React.useState(60);
  const [pan, setPan] = React.useState(-0.3);
  return (
    <div className="flex items-start gap-10">
      <Row label="Knob sizes">
        <div className="flex items-end gap-6">
          <KnobCol label="Rate" value={`${rate.toFixed(1)} Hz`}><Knob label="Rate" size={28} value={rate} onChange={setRate} min={0.1} max={20} step={0.1} /></KnobCol>
          <KnobCol label="Depth" value={`${depth} %`}><Knob label="Depth" size={36} value={depth} onChange={setDepth} min={0} max={100} step={1} /></KnobCol>
          <KnobCol label="Pan" value={pan === 0 ? "C" : pan < 0 ? `L ${Math.round(-pan * 100)}` : `R ${Math.round(pan * 100)}`}><Knob label="Pan" size={44} value={pan} onChange={setPan} min={-1} max={1} step={0.01} origin={0} /></KnobCol>
          <KnobCol label="Disabled" value="—"><Knob label="Disabled" size={36} value={30} onChange={() => {}} min={0} max={100} disabled /></KnobCol>
        </div>
      </Row>
      <Row label="Meters">
        <div className="flex h-36 items-stretch gap-3">
          <Meter label="In" level={-18} peak={-12} thickness={4} scale />
          <Meter label="Out" level={-9} peak={-4} thickness={6} scale />
          <Meter label="Hot" level={-1} peak={1.5} thickness={6} />
          <div className="flex w-56 flex-col justify-end gap-2">
            <Meter label="Horizontal" level={-14} peak={-9} orientation="horizontal" thickness={6} scale />
            <Meter label="Horizontal quiet" level={-30} orientation="horizontal" thickness={4} />
          </div>
        </div>
      </Row>
    </div>
  );
}

function KnobCol({ label, value, children }: { label: string; value: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col items-center gap-1.5">
      {children}
      <span className="text-xs text-gray-950/60">{label}</span>
      <span className="text-xs font-medium tabular">{value}</span>
    </div>
  );
}

/* Layout helpers */

function Section({ title, note, children }: { title: string; note?: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-4">
      <div className="flex items-baseline gap-3 border-b border-alpha/5 pb-2">
        <h2 className="text-base font-medium">{title}</h2>
        {note && <p className="text-xs text-gray-950/50">{note}</p>}
      </div>
      {children}
    </section>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-start gap-3">
      <span className="w-28 shrink-0 pt-1.5 text-xs text-gray-950/50">{label}</span>
      <div className="flex flex-wrap items-center gap-2">{children}</div>
    </div>
  );
}

type DemoState = "default" | "hover" | "focus" | "disabled";
const STATES: DemoState[] = ["default", "hover", "focus", "disabled"];

function StateGrid({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <div className="grid grid-cols-[7rem_repeat(4,1fr)] gap-3">
        <span />
        {STATES.map((s) => (
          <span key={s} className="text-2xs font-semibold uppercase tracking-[0.08em] text-gray-950/40">{s}</span>
        ))}
      </div>
      {children}
    </div>
  );
}

function StateRow({ label, children }: { label: string; children: (state: DemoState) => React.ReactNode }) {
  return (
    <div className="grid grid-cols-[7rem_repeat(4,1fr)] items-center gap-3">
      <span className="text-xs text-gray-950/50">{label}</span>
      {STATES.map((s) => (
        <div key={s} className={cn("flex", s === "hover" && "force-hover", s === "focus" && "force-focus")}>
          {children(s)}
        </div>
      ))}
    </div>
  );
}
