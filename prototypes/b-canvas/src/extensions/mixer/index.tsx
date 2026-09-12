import * as React from "react";
import type { ToolDefinition, ToolViewProps } from "@/core/sdk";
import { Button, Knob, Meter, NumericInput, Separator, Slider, Switch } from "@/ui";

/* ---------------------------------------------------------------- state --
   One module-level store so the Strips and Sends views of an instance always
   show the same channels. Levels are dBFS, pan is -100 (L) to 100 (R). */

interface Channel {
  id: string;
  name: string;
  pan: number;
  level: number;
  mute: boolean;
  solo: boolean;
  post: boolean;
  levels: number[];
  peaks: number[];
}

interface MixerState {
  channels: Channel[];
  master: { level: number; levels: number[]; peaks: number[] };
}

let state: MixerState = {
  channels: [
    { id: "in-1", name: "Field rec.", pan: -12, level: -8.5, mute: false, solo: false, post: true, levels: [-14, -11], peaks: [-7, -5] },
    { id: "in-2", name: "Drone", pan: 8, level: -6, mute: false, solo: false, post: true, levels: [-9, -12], peaks: [-4, -6] },
    { id: "in-3", name: "Aux", pan: 0, level: -21.5, mute: true, solo: false, post: false, levels: [-46, -44], peaks: [-38, -36] },
  ],
  master: { level: -3, levels: [-9, -12], peaks: [-4, -6] },
};

const listeners = new Set<() => void>();
const subscribe = (fn: () => void) => {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
};
const getSnapshot = () => state;
const useMixer = () => React.useSyncExternalStore(subscribe, getSnapshot);

function setChannel(id: string, patch: Partial<Channel>) {
  state = { ...state, channels: state.channels.map((c) => (c.id === id ? { ...c, ...patch } : c)) };
  for (const fn of listeners) fn();
}

function setMaster(level: number) {
  state = { ...state, master: { ...state.master, level } };
  for (const fn of listeners) fn();
}

const panLabel = (p: number) => (Math.round(p) === 0 ? "C" : p < 0 ? `L${Math.abs(Math.round(p))}` : `R${Math.round(p)}`);
const dbLabel = (db: number) => `${db.toFixed(1)} dB`;

const MIN_DB = -60;
const MAX_DB = 6;

/* ----------------------------------------------------------- strips view --*/

function ChannelStrip({ ch }: { ch: Channel }) {
  return (
    <div className="flex min-w-0 flex-1 flex-col items-center gap-2">
      <span className="w-full truncate text-center font-medium text-gray-950 text-xs">{ch.name}</span>

      <div className="flex flex-col items-center gap-1">
        <Knob
          size={28}
          bipolar
          min={-100}
          max={100}
          value={ch.pan}
          onChange={(v) => setChannel(ch.id, { pan: Math.round(v) })}
          label={`${ch.name} pan`}
        />
        <span className="tabular text-2xs text-gray-800">{panLabel(ch.pan)}</span>
      </div>

      <div className="flex h-[116px] items-stretch gap-1.5">
        <Slider
          orientation="vertical"
          size="sm"
          min={MIN_DB}
          max={MAX_DB}
          step={0.5}
          value={ch.level}
          onChange={(v) => setChannel(ch.id, { level: v })}
          label={`${ch.name} level`}
          aria-valuetext={dbLabel(ch.level)}
        />
        <Meter size="sm" levels={ch.levels} peaks={ch.peaks} label={`${ch.name} output meter`} />
      </div>

      <span className="tabular text-gray-950 text-xs">{dbLabel(ch.level)}</span>

      <div className="flex items-center gap-1">
        <Button
          size="xs"
          variant="ghost-muted"
          className="w-6 px-0"
          active={ch.mute}
          aria-label={`Mute ${ch.name}`}
          title={`Mute ${ch.name}`}
          onClick={() => setChannel(ch.id, { mute: !ch.mute })}
        >
          M
        </Button>
        <Button
          size="xs"
          variant="ghost-muted"
          className="w-6 px-0"
          active={ch.solo}
          aria-label={`Solo ${ch.name}`}
          title={`Solo ${ch.name}`}
          onClick={() => setChannel(ch.id, { solo: !ch.solo })}
        >
          S
        </Button>
      </div>
    </div>
  );
}

function MasterStrip({ master }: { master: MixerState["master"] }) {
  const [dim, setDim] = React.useState(false);
  return (
    <div className="flex w-[68px] shrink-0 flex-col items-center gap-2">
      <span className="w-full truncate text-center font-medium text-gray-950 text-xs">Main</span>

      <div className="flex h-[46px] flex-col items-center justify-center gap-0.5">
        <span className="text-2xs text-gray-700">Out 1–2</span>
        <span className="tabular text-2xs text-gray-900">−14.2 LUFS</span>
      </div>

      <div className="flex h-[116px] items-stretch gap-1.5">
        <Slider
          orientation="vertical"
          size="sm"
          min={MIN_DB}
          max={MAX_DB}
          step={0.5}
          value={master.level}
          onChange={setMaster}
          label="Main out level"
          aria-valuetext={dbLabel(master.level)}
        />
        <Meter size="md" levels={master.levels} peaks={master.peaks} label="Main out meter" />
      </div>

      <span className="tabular text-gray-950 text-xs">{dbLabel(master.level)}</span>

      <Button
        size="xs"
        variant="ghost-muted"
        className="px-1.5"
        active={dim}
        aria-label="Dim the main output"
        title="Dim the main output"
        onClick={() => setDim(!dim)}
      >
        DIM
      </Button>
    </div>
  );
}

function StripsView(_props: ToolViewProps) {
  const { channels, master } = useMixer();
  return (
    <div className="flex items-stretch gap-2">
      {channels.map((ch) => (
        <ChannelStrip key={ch.id} ch={ch} />
      ))}
      <Separator orientation="vertical" className="mx-0.5" />
      <MasterStrip master={master} />
    </div>
  );
}

/* ------------------------------------------------------------ sends view --*/

function SendsView(_props: ToolViewProps) {
  const { channels } = useMixer();
  const cols = "grid grid-cols-[1fr_70px_46px_28px] items-center gap-x-2";

  return (
    <div className="flex flex-col gap-2">
      <div className={cols}>
        <span className="text-2xs text-gray-700">Channel</span>
        <span className="text-2xs text-gray-700">Level</span>
        <span className="text-2xs text-gray-700">Pan</span>
        <span className="text-center text-2xs text-gray-700">Post</span>
      </div>

      <div className="flex flex-col gap-1.5">
        {channels.map((ch) => (
          <div key={ch.id} className={cols}>
            <span className="truncate font-medium text-gray-950 text-xs">{ch.name}</span>
            <NumericInput
              size="xs"
              block
              label={`${ch.name} send level`}
              value={ch.level}
              onChange={(v) => setChannel(ch.id, { level: v })}
              min={MIN_DB}
              max={MAX_DB}
              step={0.1}
              precision={1}
              unit="dB"
            />
            <NumericInput
              size="xs"
              block
              label={`${ch.name} send pan`}
              value={ch.pan}
              onChange={(v) => setChannel(ch.id, { pan: Math.round(v) })}
              min={-100}
              max={100}
              step={1}
              precision={0}
            />
            <div className="flex justify-center">
              <Switch
                size="xs"
                accent
                checked={ch.post}
                onCheckedChange={(v) => setChannel(ch.id, { post: v })}
                label={`${ch.name} post-fader send`}
              />
            </div>
          </div>
        ))}
      </div>

      <Separator />
      <span className="text-2xs text-gray-700">Post sends follow the channel fader and mute.</span>
    </div>
  );
}

export const tool: ToolDefinition = {
  type: "mixer",
  name: "Mixer",
  description: "Balances the instruments feeding it and sends one stereo mix to the main output.",
  accent: "teal",
  inputs: [
    { id: "in-1", label: "In 1", kind: "audio" },
    { id: "in-2", label: "In 2", kind: "audio" },
    { id: "in-3", label: "In 3", kind: "audio" },
    { id: "level-mod", label: "Level", kind: "mod" },
  ],
  outputs: [{ id: "main", label: "Main out", kind: "audio" }],
  views: [
    { id: "strips", label: "Strips", width: 300, component: StripsView },
    { id: "sends", label: "Sends", width: 300, component: SendsView },
  ],
};
