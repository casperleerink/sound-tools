import { ArrowLeft, Copy, Pause, Play, Plus, Settings2, Square, X } from "lucide-react";
import * as React from "react";
import { wirePath } from "@/core/Canvas";
import type { PortDef } from "@/core/sdk";
import { type Accent, accentStyle, accents } from "@/lib/accent";
import { cn } from "@/lib/cn";
import {
  Badge,
  Button,
  type ButtonVariant,
  Field,
  IconButton,
  Kbd,
  KbdShortcut,
  Knob,
  Meter,
  NumericInput,
  SegmentedControl,
  Separator,
  Slider,
  Switch,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  ToolFrame,
  Tooltip,
} from "@/ui";

/* Every primitive, every variant, every state. Forced states use data-hover /
   data-focus, which the hover and focus-visible variants also match. */
export function Gallery() {
  return (
    <div className="h-full overflow-y-auto bg-gray-50 text-gray-950" style={accentStyle("teal")}>
      <div className="app-drag-region sticky top-0 z-10 flex h-11 items-center gap-3 border-alpha/10 border-b bg-gray-50/90 pr-4 pl-[76px] backdrop-blur-md">
        <a href="#/" className="app-no-drag focus-ring flex h-7 items-center gap-1 rounded-lg px-2 text-gray-900 text-sm hover:bg-alpha/5 hover:text-gray-950">
          <ArrowLeft className="size-4" />
          Workspace
        </a>
        <Separator orientation="vertical" className="h-4" />
        <span className="font-medium text-sm">Primitives</span>
        <span className="text-gray-700 text-xs">UI SDK · Catppuccin Mocha on Hooman</span>
        <span className="flex-1" />
        <span className="text-gray-700 text-xs">Accent for this page: teal</span>
      </div>

      <div className="mx-auto flex max-w-[1180px] flex-col gap-12 px-8 py-8 pb-24">
        <Colours />
        <Typography />
        <Buttons />
        <Badges />
        <Controls />
        <Sliders />
        <ToolCards />
      </div>
    </div>
  );
}

function Section({ title, hint, children }: { title: string; hint?: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-4">
      <div className="flex items-baseline gap-3">
        <h2 className="font-semibold text-base">{title}</h2>
        {hint ? <span className="text-gray-700 text-xs">{hint}</span> : null}
      </div>
      {children}
    </section>
  );
}

function Row({ label, children, className }: { label: string; children: React.ReactNode; className?: string }) {
  return (
    <div className={cn("flex items-center gap-3", className)}>
      <span className="w-24 shrink-0 text-gray-700 text-xs">{label}</span>
      <div className="flex flex-wrap items-center gap-2">{children}</div>
    </div>
  );
}

function Panel({ children, className }: { children: React.ReactNode; className?: string }) {
  return <div className={cn("flex flex-col gap-3 rounded-[10px] border border-alpha/10 bg-gray-200 p-4", className)}>{children}</div>;
}

/* ---------- colours ---------- */

const grays = [
  ["50", "crust", "window"],
  ["100", "mantle", "canvas"],
  ["200", "base", "cards"],
  ["300", "surface0"],
  ["400", "surface1"],
  ["500", "surface2"],
  ["600", "overlay0"],
  ["700", "overlay1"],
  ["800", "overlay2"],
  ["900", "subtext0"],
  ["950", "text"],
] as const;

function Colours() {
  return (
    <Section title="Colour" hint="Hooman scale mapped to Mocha. gray-50 is the darkest surface, gray-950 the text.">
      <div className="grid grid-cols-11 gap-1.5">
        {grays.map(([n, name, use]) => (
          <div key={n} className="flex flex-col gap-1.5">
            <div className="h-12 rounded-md border border-alpha/10" style={{ background: `var(--color-gray-${n})` }} />
            <span className="tabular text-2xs text-gray-900">gray-{n}</span>
            <span className="text-2xs text-gray-700">
              {name}
              {use ? ` · ${use}` : ""}
            </span>
          </div>
        ))}
      </div>
      <div className="grid grid-cols-7 gap-1.5 lg:grid-cols-14">
        {(Object.keys(accents) as Accent[]).map((a) => (
          <div key={a} className="flex flex-col gap-1.5">
            <div className="h-10 rounded-md" style={{ background: accents[a] }} />
            <span className="text-2xs text-gray-900">{a}</span>
          </div>
        ))}
      </div>
      <Row label="Semantic">
        <Badge variant="agent" dot>
          agent · lavender
        </Badge>
        <Badge variant="green" dot>
          play · green
        </Badge>
        <Badge variant="orange" dot>
          warning · peach
        </Badge>
        <Badge variant="red" dot>
          error · red
        </Badge>
        <Badge variant="accent-subtle" dot>
          instance accent
        </Badge>
      </Row>
    </Section>
  );
}

/* ---------- type ---------- */

function Typography() {
  return (
    <Section title="Typography" hint='InterDisplay, features "ss03" "cv01". Numbers use the tabular utility.'>
      <Panel className="gap-2">
        <p className="text-xl">Three voices, independent rhythms</p>
        <p className="text-lg">Three voices, independent rhythms</p>
        <p className="text-base">Body base 16 · Stretch a pattern by dragging its right edge.</p>
        <p className="text-sm">Body sm 14 · Voice B is 2.5 s now. That was a project edit, so it applied instantly.</p>
        <p className="font-medium text-sm">Label sm medium · Frequency</p>
        <p className="text-gray-900 text-xs">Meta xs 12 · gray-900 for labels</p>
        <p className="text-2xs text-gray-700">Meta 2xs 10 · gray-700 for hints</p>
        <div className="flex gap-6">
          <span className="tabular font-mono text-sm">00:01:24.36</span>
          <span className="tabular text-sm">2.00 s · 440.00 Hz · −6.0 dB</span>
          <span className="font-mono text-gray-900 text-xs">extensions/step-seq/src/lib.rs</span>
        </div>
      </Panel>
    </Section>
  );
}

/* ---------- buttons ---------- */

const buttonGroups: { name: string; variants: ButtonVariant[] }[] = [
  { name: "Neutral", variants: ["primary", "subtle", "outline", "ghost", "ghost-muted"] },
  { name: "Accent", variants: ["accent", "accent-subtle", "accent-outline", "accent-ghost"] },
  { name: "Agent", variants: ["agent", "agent-subtle", "agent-ghost"] },
  { name: "Green", variants: ["green", "green-subtle", "green-outline", "green-ghost"] },
  { name: "Orange", variants: ["orange", "orange-subtle", "orange-ghost"] },
  { name: "Red", variants: ["red", "red-subtle", "red-outline", "red-ghost"] },
  { name: "Blue", variants: ["blue", "blue-subtle", "blue-ghost"] },
];

const states = ["default", "hover", "focus", "active", "disabled"] as const;

function Buttons() {
  return (
    <Section title="Buttons" hint="Sizes 24 / 28 / 32 / 40. Solid fills use dark ink text. States left to right: default, hover, focus, active, disabled.">
      <Panel>
        <div className="grid grid-cols-[6rem_repeat(5,minmax(0,1fr))] items-center gap-x-3 gap-y-2">
          <span />
          {states.map((s) => (
            <span key={s} className="text-2xs text-gray-700 capitalize">
              {s}
            </span>
          ))}
          {buttonGroups.flatMap((g) =>
            g.variants.map((v) => (
              <React.Fragment key={v}>
                <span className="text-gray-800 text-xs">{v}</span>
                {states.map((s) => (
                  <div key={s}>
                    <Button
                      variant={v}
                      size="sm"
                      data-hover={s === "hover" ? "" : undefined}
                      data-focus={s === "focus" ? "" : undefined}
                      active={s === "active"}
                      disabled={s === "disabled"}
                    >
                      Button
                    </Button>
                  </div>
                ))}
              </React.Fragment>
            )),
          )}
        </div>
      </Panel>
      <Panel>
        <Row label="Sizes">
          <Button variant="primary" size="xs">
            Extra small
          </Button>
          <Button variant="primary" size="sm">
            Small
          </Button>
          <Button variant="primary" size="md">
            Medium
          </Button>
          <Button variant="primary" size="lg">
            Large
          </Button>
        </Row>
        <Row label="With icon">
          <Button variant="subtle" size="sm">
            <Plus />
            <span>Add tool</span>
          </Button>
          <Button variant="green-subtle" size="md">
            <Play className="fill-current" />
            <span>Play</span>
          </Button>
          <Button variant="outline" size="sm" rounded>
            <Settings2 />
            <span>Settings</span>
          </Button>
        </Row>
        <Row label="Icon buttons">
          <IconButton label="Close" size="xs">
            <X />
          </IconButton>
          <IconButton label="Copy" size="sm">
            <Copy />
          </IconButton>
          <IconButton label="Play" size="md" variant="green-subtle" rounded>
            <Play className="fill-current" />
          </IconButton>
          <IconButton label="Pause" size="md" variant="subtle" rounded>
            <Pause className="fill-current" />
          </IconButton>
          <IconButton label="Stop" size="lg" variant="outline">
            <Square className="fill-current" />
          </IconButton>
          <IconButton label="Disabled" size="md" disabled>
            <X />
          </IconButton>
        </Row>
        <Row label="Kbd">
          <Kbd>⌘</Kbd>
          <Kbd>K</Kbd>
          <KbdShortcut shortcut="mod+shift+z" />
          <KbdShortcut shortcut="space" />
          <KbdShortcut shortcut="mod+enter" />
        </Row>
        <Row label="Tooltip">
          <Tooltip content="Play" shortcut="space" open>
            <IconButton label="Play" size="md" variant="subtle">
              <Play className="fill-current" />
            </IconButton>
          </Tooltip>
          <span className="w-12" />
          <Tooltip content="Right side" side="right" open>
            <Button variant="subtle" size="sm">
              Hover me
            </Button>
          </Tooltip>
        </Row>
      </Panel>
    </Section>
  );
}

/* ---------- badges ---------- */

function Badges() {
  const variants = ["primary", "subtle", "muted", "outline", "accent", "accent-subtle", "accent-outline", "agent", "green", "orange", "red", "blue", "yellow"] as const;
  return (
    <Section title="Badges" hint="Sizes 16 / 20 / 24 / 28.">
      <Panel>
        <Row label="Variants">
          {variants.map((v) => (
            <Badge key={v} variant={v}>
              {v}
            </Badge>
          ))}
        </Row>
        <Row label="Sizes">
          <Badge size="xs">xs</Badge>
          <Badge size="sm">sm</Badge>
          <Badge size="md">md</Badge>
          <Badge size="lg">lg</Badge>
          <Badge size="sm" rounded variant="accent-subtle" dot>
            rounded · dot
          </Badge>
          <Badge size="sm" variant="accent-outline" icon={<Copy />}>
            2 views
          </Badge>
          <Badge size="xs" variant="green">
            no build
          </Badge>
        </Row>
      </Panel>
    </Section>
  );
}

/* ---------- controls ---------- */

function Controls() {
  const [seg, setSeg] = React.useState("pattern");
  const [tab, setTab] = React.useState("voice");
  const [on, setOn] = React.useState(true);
  const [n, setN] = React.useState(440);
  const [m, setM] = React.useState(2.5);
  return (
    <Section title="Controls" hint="Segmented control, tabs, switch, separator, labelled field, numeric input (drag to change, click to type).">
      <div className="grid grid-cols-2 gap-4">
        <Panel>
          <Row label="Segmented xs">
            <SegmentedControl size="xs" label="View" value={seg} onValueChange={setSeg} options={[{ value: "pattern", label: "Patterns" }, { value: "voices", label: "Voices" }]} />
            <SegmentedControl size="xs" label="Grid" value="16" onValueChange={() => {}} options={[{ value: "8", label: "1/8" }, { value: "16", label: "1/16" }, { value: "free", label: "Free" }]} />
          </Row>
          <Row label="Segmented sm">
            <SegmentedControl size="sm" label="Mode" value="loop" onValueChange={() => {}} options={[{ value: "once", label: "Once" }, { value: "loop", label: "Loop" }, { value: "pp", label: "Ping-pong" }]} />
          </Row>
          <Row label="Segmented md">
            <SegmentedControl size="md" rounded label="Provider" value="anthropic" onValueChange={() => {}} options={[{ value: "anthropic", label: "Anthropic" }, { value: "openai", label: "OpenAI" }, { value: "local", label: "Local" }]} />
          </Row>
          <Row label="Tabs">
            <Tabs value={tab} onValueChange={setTab}>
              <TabsList label="Sections">
                <TabsTrigger value="voice">Voice</TabsTrigger>
                <TabsTrigger value="mod">Modulation</TabsTrigger>
                <TabsTrigger value="routing">Routing</TabsTrigger>
                <TabsTrigger value="off" disabled>
                  Disabled
                </TabsTrigger>
              </TabsList>
              <TabsContent value="voice" className="mt-2 text-gray-800 text-xs">
                Voice panel
              </TabsContent>
              <TabsContent value="mod" className="mt-2 text-gray-800 text-xs">
                Modulation panel
              </TabsContent>
              <TabsContent value="routing" className="mt-2 text-gray-800 text-xs">
                Routing panel
              </TabsContent>
            </Tabs>
          </Row>
          <Row label="Switch">
            <Switch label="Loop" size="xs" checked={on} onCheckedChange={setOn} />
            <Switch label="Loop" size="sm" checked={on} onCheckedChange={setOn} />
            <Switch label="Loop accent" size="sm" accent checked={on} onCheckedChange={setOn} />
            <Switch label="Off" size="sm" checked={false} onCheckedChange={() => {}} />
            <Switch label="Disabled" size="sm" checked disabled onCheckedChange={() => {}} />
            <Switch label="Focused" size="sm" checked onCheckedChange={() => {}} data-focus="" />
          </Row>
          <Row label="Separator" className="items-stretch">
            <div className="flex h-6 items-center gap-2 text-gray-800 text-xs">
              A <Separator orientation="vertical" /> B
            </div>
            <div className="flex w-40 flex-col gap-2 text-gray-800 text-xs">
              <span>Above</span>
              <Separator />
              <span>Below</span>
            </div>
          </Row>
        </Panel>
        <Panel>
          <Field label="Frequency" value="A4">
            <NumericInput value={n} onChange={setN} min={20} max={20000} precision={2} unit="Hz" block />
          </Field>
          <Field label="Length" layout="row" value="voice B">
            <NumericInput value={m} onChange={setM} min={0.1} max={16} precision={2} unit="s" size="sm" />
          </Field>
          <Row label="Sizes">
            <NumericInput label="Steps" value={5} onChange={() => {}} size="xs" />
            <NumericInput label="Level" value={-6} onChange={() => {}} size="sm" precision={1} unit="dB" />
            <NumericInput label="Cutoff" value={1200} onChange={() => {}} size="md" unit="Hz" />
          </Row>
          <Row label="States">
            <NumericInput label="Focused" value={12} onChange={() => {}} size="sm" data-focus="" />
            <NumericInput label="Hover" value={12} onChange={() => {}} size="sm" data-hover="" />
            <NumericInput label="Disabled" value={12} onChange={() => {}} size="sm" disabled unit="%" />
          </Row>
          <Field label="Humanise" value="12 %" hint="Adds up to this much random timing to every step.">
            <Slider value={0.12} onChange={() => {}} />
          </Field>
        </Panel>
      </div>
    </Section>
  );
}

/* ---------- sliders, knobs, meters ---------- */

function Sliders() {
  const [a, setA] = React.useState(0.62);
  const [b, setB] = React.useState(-0.3);
  const [k, setK] = React.useState(0.7);
  return (
    <Section title="Sliders, knobs, meters" hint="Fill and arc use the instance accent. Ghost marker shows the modulated value.">
      <div className="grid grid-cols-3 gap-4">
        <Panel>
          <Field label="Horizontal md" value={`${Math.round(a * 100)} %`}>
            <Slider value={a} onChange={setA} />
          </Field>
          <Field label="Horizontal sm" value={`${Math.round(a * 100)} %`}>
            <Slider size="sm" value={a} onChange={setA} />
          </Field>
          <Field label="Bipolar (pan)" value={b < 0 ? `L${Math.round(-b * 50)}` : b > 0 ? `R${Math.round(b * 50)}` : "C"}>
            <Slider bipolar min={-1} max={1} value={b} onChange={setB} />
          </Field>
          <Field label="Modulated" value="base 62 % · now 81 %">
            <Slider value={a} onChange={setA} modulated={Math.min(1, a + 0.19)} />
          </Field>
          <Field label="Stepped, no fill" value="3 of 8">
            <Slider fill={false} min={0} max={8} step={1} value={3} onChange={() => {}} />
          </Field>
          <Field label="Focused">
            <Slider value={0.4} onChange={() => {}} data-focus="" />
          </Field>
          <Field label="Disabled">
            <Slider value={0.4} onChange={() => {}} disabled />
          </Field>
        </Panel>
        <Panel>
          <span className="text-gray-700 text-xs">Vertical sliders and meters</span>
          <div className="flex h-40 items-stretch gap-5 px-2">
            <Slider orientation="vertical" value={a} onChange={setA} label="Level A" />
            <Slider orientation="vertical" size="sm" value={0.4} onChange={() => {}} label="Level B" />
            <Slider orientation="vertical" bipolar min={-1} max={1} value={b} onChange={setB} label="Pan" />
            <Separator orientation="vertical" />
            <Meter levels={[-9]} peaks={[-4]} label="Mono meter" />
            <Meter levels={[-9, -12]} peaks={[-4, -6]} label="Stereo meter" />
            <Meter size="sm" levels={[-2, -1]} peaks={[0, -0.5]} label="Hot meter" />
            <Meter levels={[-40]} label="Quiet meter" />
          </div>
          <div className="flex flex-col gap-2">
            <Meter orientation="horizontal" levels={[-9, -12]} peaks={[-4, -6]} label="Horizontal meter" />
            <Meter orientation="horizontal" size="sm" levels={[-18]} label="Horizontal sm" />
          </div>
        </Panel>
        <Panel>
          <span className="text-gray-700 text-xs">Knobs 28 / 36 / 44 · bipolar · focused · disabled</span>
          <div className="flex items-end gap-5">
            <KnobField label="Swing" value={k} onChange={setK} size={28} />
            <KnobField label="Attack" value={k} onChange={setK} size={36} />
            <KnobField label="Release" value={k} onChange={setK} size={44} />
          </div>
          <div className="flex items-end gap-5">
            <div className="flex flex-col items-center gap-1">
              <Knob min={-1} max={1} bipolar value={b} onChange={setB} label="Pan" />
              <span className="text-2xs text-gray-800">Pan</span>
            </div>
            <div className="flex flex-col items-center gap-1">
              <Knob value={0.3} onChange={() => {}} label="Focused" data-focus="" />
              <span className="text-2xs text-gray-800">Focused</span>
            </div>
            <div className="flex flex-col items-center gap-1">
              <Knob value={0.3} onChange={() => {}} label="Disabled" disabled />
              <span className="text-2xs text-gray-800">Disabled</span>
            </div>
          </div>
        </Panel>
      </div>
    </Section>
  );
}

function KnobField({ label, value, onChange, size }: { label: string; value: number; onChange: (v: number) => void; size: 28 | 36 | 44 }) {
  return (
    <div className="flex flex-col items-center gap-1">
      <Knob value={value} onChange={onChange} size={size} label={label} />
      <span className="text-2xs text-gray-800">{label}</span>
      <span className="tabular text-2xs text-gray-700">{Math.round(value * 100)}</span>
    </div>
  );
}

/* ---------- tool cards ---------- */

const demoInputs: PortDef[] = [
  { id: "gate", label: "Gate", kind: "event" },
  { id: "pitch", label: "Pitch", kind: "mod" },
  { id: "in", label: "In", kind: "audio" },
];
const demoOutputs: PortDef[] = [{ id: "out", label: "Out", kind: "audio" }];

function ToolCards() {
  const [view, setView] = React.useState("main");
  const [v, setV] = React.useState(0.5);
  const wires: { kind: "audio" | "event" | "mod"; label: string; width: number; dash?: string }[] = [
    { kind: "audio", label: "Audio · 2.5 px solid", width: 2.5 },
    { kind: "event", label: "Events · 1.5 px dashed", width: 1.5, dash: "6 4" },
    { kind: "mod", label: "Modulation · 1.5 px dotted", width: 1.5, dash: "1.5 4" },
  ];
  return (
    <Section title="Tool card and ports" hint="Header: accent dot, instance name, tool type, shared-view marker, view switcher, open another view, close. Ports: disc = audio, ring = events, diamond = modulation.">
      <div className="canvas-grid flex flex-wrap items-start gap-12 rounded-[10px] border border-alpha/10 p-8 pl-12">
        <ToolFrame
          instanceName="Drone"
          typeName="Additive"
          accent="blue"
          views={[{ id: "main", label: "Partials" }, { id: "voice", label: "Voice" }]}
          activeView={view}
          onViewChange={setView}
          openViews={2}
          inputs={demoInputs}
          outputs={demoOutputs}
          connectedPorts={new Set(["gate", "out"])}
          width={280}
        >
          <div className="flex flex-col gap-3">
            <Field label="Fundamental" value="220.00 Hz">
              <Slider value={v} onChange={setV} />
            </Field>
            <Field label="Level" layout="row">
              <NumericInput value={-6} onChange={() => {}} precision={1} unit="dB" size="xs" />
            </Field>
          </div>
        </ToolFrame>
        <ToolFrame instanceName="Output" typeName="Mixer" accent="teal" views={[{ id: "strips", label: "Strips" }]} activeView="strips" onViewChange={() => {}} inputs={[{ id: "in-1", label: "In 1", kind: "audio" }]} width={240} selected>
          <div className="text-gray-800 text-xs">Selected card: accent border.</div>
        </ToolFrame>
        <div className="flex flex-col gap-2 rounded-lg border border-alpha/10 bg-gray-200 p-3">
          <span className="text-gray-700 text-xs">Wires, coloured by the source instance</span>
          <svg width={220} height={110} aria-hidden>
            {wires.map((w, i) => {
              const a = { x: 8, y: 16 + i * 36 };
              const b = { x: 212, y: 24 + i * 36 };
              return (
                <g key={w.kind}>
                  <path d={wirePath(a, b)} fill="none" stroke="var(--color-gray-100)" strokeWidth={w.width + 3} strokeLinecap="round" />
                  <path d={wirePath(a, b)} fill="none" stroke={accents[i === 0 ? "yellow" : i === 1 ? "peach" : "pink"]} strokeWidth={w.width} strokeDasharray={w.dash} strokeLinecap="round" />
                </g>
              );
            })}
          </svg>
          <div className="flex flex-col gap-0.5 text-2xs text-gray-800">
            {wires.map((w) => (
              <span key={w.kind}>{w.label}</span>
            ))}
          </div>
        </div>
      </div>
    </Section>
  );
}
