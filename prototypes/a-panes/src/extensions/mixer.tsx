import {
  Badge,
  Button,
  Knob,
  Meter,
  NumericInput,
  Panel,
  Select,
  Separator,
  Slider,
  ToolView,
  cn,
  useInstanceState,
} from "@/sdk";
import type { ExtensionDef, Port, ToolViewProps } from "@/sdk";

/* ------------------------------------------------------------------ model */

/** Faders and meters share this dB range, so the scale column lines up with both. */
const MIN_DB = -60;
const MAX_DB = 6;
const SCALE_MARKS = [0, -6, -12, -24];

/** Same percentage math the Slider uses, so labels sit on the ticks. */
const pctOf = (db: number) => ((db - MIN_DB) / (MAX_DB - MIN_DB)) * 100;

/** Minus sign that matches the rest of the UI. */
const formatDb = (v: number) => v.toFixed(1).replace("-", "−");

function formatPan(pan: number) {
  if (pan === 0) return "C";
  return pan < 0 ? `L ${-pan}` : `R ${pan}`;
}

interface Channel {
  id: string;
  name: string;
  /** Set when the source reaches the mixer through another tool. */
  via?: string;
  level: number;
  pan: number;
  /** Fake meter reading, one per channel so the strips are telling apart. */
  meter: { level: number; peak: number };
}

const CHANNELS: Channel[] = [
  { id: "ch-1", name: "Tone A", via: "Tremolo", level: -4, pan: -20, meter: { level: -14, peak: -9 } },
  { id: "ch-2", name: "Tone B", level: -12, pan: 0, meter: { level: -20, peak: -15 } },
  { id: "ch-3", name: "Tone C", level: -7, pan: 35, meter: { level: -11, peak: -6 } },
];

const OUTPUTS = ["MacBook Pro Speakers", "Scarlett 2i2", "Aggregate device"] as const;
type Output = (typeof OUTPUTS)[number];

/* -------------------------------------------------------------- fragments */

/** dB labels down the left of a fader. Absolutely placed on the same percentages as the ticks. */
function ScaleColumn() {
  return (
    <div aria-hidden className="relative w-4 shrink-0">
      {SCALE_MARKS.map((db) => (
        <span
          key={db}
          className="absolute right-0 translate-y-1/2 text-2xs text-gray-950/40 tabular"
          style={{ bottom: `${pctOf(db)}%` }}
        >
          {db === 0 ? "0" : `−${Math.abs(db)}`}
        </span>
      ))}
    </div>
  );
}

/** Fixed-height rows keep every strip's fader, readout and buttons on the same lines. */
const HEAD_ROW = "flex h-8 shrink-0 flex-col justify-center gap-0.5";
const KNOB_ROW = "flex h-7 shrink-0 items-center justify-center gap-1";
const FADER_ROW = "flex min-h-[120px] flex-1 items-stretch justify-center gap-1";
const STRIP = "flex w-17 shrink-0 flex-col gap-1.5 py-1.5";

/* ----------------------------------------------------------- channel strip */

function ChannelStrip({
  instanceId,
  index,
  channel,
}: {
  instanceId: string;
  index: number;
  channel: Channel;
}) {
  const [level, setLevel] = useInstanceState(instanceId, `${channel.id}:level`, channel.level);
  const [pan, setPan] = useInstanceState(instanceId, `${channel.id}:pan`, channel.pan);
  const [mute, setMute] = useInstanceState(instanceId, `${channel.id}:mute`, false);
  const [solo, setSolo] = useInstanceState(instanceId, `${channel.id}:solo`, false);

  return (
    <div className={STRIP}>
      <div className={HEAD_ROW}>
        <div className="flex min-w-0 items-center gap-1">
          <Badge variant="subtle" size="xs" tabular>
            {index}
          </Badge>
          <span className="truncate text-xs font-medium text-gray-950">{channel.name}</span>
        </div>
        {channel.via && <span className="truncate text-xs text-gray-950/40">via {channel.via}</span>}
      </div>

      <div className={KNOB_ROW}>
        <Knob
          value={pan}
          onChange={setPan}
          min={-100}
          max={100}
          step={1}
          origin={0}
          size={28}
          label={`Pan ${channel.name}`}
        />
        <span className="w-8 shrink-0 text-2xs text-gray-950/60 tabular">{formatPan(pan)}</span>
      </div>

      <div className={cn(FADER_ROW, mute && "opacity-40")}>
        <ScaleColumn />
        <Slider
          orientation="vertical"
          value={level}
          onChange={setLevel}
          min={MIN_DB}
          max={MAX_DB}
          step={0.5}
          ticks={SCALE_MARKS}
          thickness={4}
          label={`Level ${channel.name}`}
        />
        <Meter
          orientation="vertical"
          level={channel.meter.level}
          peak={channel.meter.peak}
          thickness={4}
          scale
          label={`${channel.name} output level`}
        />
      </div>

      <NumericInput
        value={level}
        onChange={setLevel}
        min={MIN_DB}
        max={MAX_DB}
        step={0.5}
        decimals={1}
        format={formatDb}
        unit="dB"
        size="xs"
        className="w-full px-1.5"
        label={`Level ${channel.name}`}
      />

      <div className="flex shrink-0 gap-1">
        <Button
          size="xs"
          variant={mute ? "orange-subtle" : "quiet"}
          className="flex-1 px-0"
          aria-label={`Mute ${channel.name}`}
          aria-pressed={mute}
          onClick={() => setMute(!mute)}
        >
          M
        </Button>
        <Button
          size="xs"
          variant={solo ? "accent-subtle" : "quiet"}
          className="flex-1 px-0"
          aria-label={`Solo ${channel.name}`}
          aria-pressed={solo}
          onClick={() => setSolo(!solo)}
        >
          S
        </Button>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------ master strip */

function MasterStrip({ instanceId }: { instanceId: string }) {
  const [level, setLevel] = useInstanceState(instanceId, "main:level", 0);

  return (
    <Panel raised className={cn(STRIP, "w-20 px-1.5")}>
      <div className={HEAD_ROW}>
        <span className="truncate text-xs font-medium text-gray-950">Main</span>
        <span className="truncate text-xs text-gray-950/40">master</span>
      </div>

      {/* Master has no pan; the empty row keeps its fader level with the channels. */}
      <div className={KNOB_ROW} aria-hidden />

      <div className={FADER_ROW}>
        <ScaleColumn />
        <Slider
          orientation="vertical"
          value={level}
          onChange={setLevel}
          min={MIN_DB}
          max={MAX_DB}
          step={0.5}
          ticks={SCALE_MARKS}
          thickness={4}
          label="Main level"
        />
        <div className="flex shrink-0 gap-0.5">
          <Meter orientation="vertical" level={-8} peak={-3} thickness={3} scale label="Main level left" />
          <Meter orientation="vertical" level={-9} peak={-4} thickness={3} scale label="Main level right" />
        </div>
      </div>

      <NumericInput
        value={level}
        onChange={setLevel}
        min={MIN_DB}
        max={MAX_DB}
        step={0.5}
        decimals={1}
        format={formatDb}
        unit="dB"
        size="xs"
        className="w-full px-1.5"
        label="Main level"
      />
      {/* Same height as the channels' mute/solo row, so the readouts line up. */}
      <div className="h-6 shrink-0" aria-hidden />
    </Panel>
  );
}

/* ---------------------------------------------------------------- the view */

function MixMain({ instance }: ToolViewProps) {
  const [output, setOutput] = useInstanceState<Output>(instance.id, "output", "MacBook Pro Speakers");

  return (
    <ToolView title={instance.name} meta={`${CHANNELS.length} channels · ${output}`}>
      <div className="flex h-full min-h-0 flex-col px-2 pt-2">
        <div className="flex min-h-0 flex-1 gap-1.5 overflow-x-auto">
          {CHANNELS.map((channel, i) => (
            <ChannelStrip key={channel.id} instanceId={instance.id} index={i + 1} channel={channel} />
          ))}
          <Separator orientation="vertical" />
          <MasterStrip instanceId={instance.id} />
        </div>
        {/* Output routing for the main bus sits under the strips so the master strip stays narrow. */}
        <div className="flex h-10 shrink-0 items-center gap-2 border-t border-alpha/5">
          <label htmlFor={`${instance.id}-output`} className="shrink-0 text-xs text-gray-950/40">
            Output
          </label>
          <Select
            id={`${instance.id}-output`}
            value={output}
            onChange={setOutput}
            options={OUTPUTS.map((o) => ({ value: o, label: o }))}
            label="Output device"
            size="xs"
            className="min-w-0 flex-1"
          />
        </div>
      </div>
    </ToolView>
  );
}

/* ------------------------------------------------------------- extension */

const PORTS: Port[] = [
  { id: "ch-1", label: "ch 1", kind: "audio", direction: "in" },
  { id: "ch-2", label: "ch 2", kind: "audio", direction: "in" },
  { id: "ch-3", label: "ch 3", kind: "audio", direction: "in" },
  { id: "main", label: "main", kind: "audio", direction: "out" },
];

export const mixerExtension: ExtensionDef = {
  name: "mixer",
  title: "Mixer",
  description: "Three input channels with pan, level, mute and solo, summed to one output.",
  tools: [
    {
      id: "mixer",
      name: "Mix",
      kind: "utility",
      accent: "flamingo",
      ports: PORTS,
      views: [{ id: "main", label: "Main", component: MixMain }],
    },
  ],
};
