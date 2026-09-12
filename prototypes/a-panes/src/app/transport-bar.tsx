import { Check, Hammer, Loader2, Pause, Play, Redo2, Speaker, Square, TriangleAlert, Undo2 } from "lucide-react";
import * as React from "react";
import { PROJECT } from "@/data/project";
import { Badge, Button, cn, IconButton, Separator, Tooltip } from "@/ui";
import { useAppState } from "./app-state";

const LOOP_LENGTH = 240; // seconds shown on the seek strip; the project itself has no fixed end

export function formatPosition(seconds: number) {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  const ms = Math.floor((seconds % 1) * 1000);
  return `${m}:${s.toString().padStart(2, "0")}.${ms.toString().padStart(3, "0")}`;
}

export function TransportBar() {
  const { transport, setTransport } = useAppState();
  const playing = transport.state === "playing";

  return (
    <footer className="flex h-9 shrink-0 items-center gap-2 border-t border-alpha/5 bg-gray-100 px-2">
      {/* Transport */}
      <div className="flex items-center gap-0.5">
        <Tooltip content={playing ? "Pause" : "Play"}>
          <IconButton
            label={playing ? "Pause" : "Play"}
            size="sm"
            variant={playing ? "green-subtle" : "ghost"}
            onClick={() => setTransport((t) => ({ ...t, state: playing ? "paused" : "playing" }))}
          >
            {playing ? <Pause className="fill-current" /> : <Play className="fill-current" />}
          </IconButton>
        </Tooltip>
        <Tooltip content="Stop">
          <IconButton
            label="Stop"
            size="sm"
            variant="ghost"
            disabled={transport.state === "stopped"}
            onClick={() => setTransport({ state: "stopped", position: 0 })}
          >
            <Square className="fill-current" />
          </IconButton>
        </Tooltip>
      </div>
      <span
        aria-live="off"
        aria-label="Project position"
        className={cn("w-[84px] shrink-0 font-mono text-xs tabular", playing ? "text-gray-950" : "text-gray-950/60")}
      >
        {formatPosition(transport.position)}
      </span>

      <SeekStrip />

      <Separator orientation="vertical" className="h-4" />

      <div className="flex items-center gap-0.5">
        <Tooltip content="Undo: Tone B level">
          <IconButton label="Undo" size="sm" variant="quiet">
            <Undo2 />
          </IconButton>
        </Tooltip>
        <Tooltip content="Nothing to redo">
          <IconButton label="Redo" size="sm" variant="quiet" disabled>
            <Redo2 />
          </IconButton>
        </Tooltip>
      </div>

      <Separator orientation="vertical" className="h-4" />

      <Tooltip content={`${PROJECT.device.sampleRate / 1000} kHz · ${PROJECT.device.bufferSize} samples`}>
        <Button variant="quiet" size="xs" className="gap-1.5 font-normal">
          <Speaker />
          <span className="max-w-44 truncate">{PROJECT.device.name}</span>
          <span className="text-gray-950/40 tabular">48k</span>
        </Button>
      </Tooltip>

      <Separator orientation="vertical" className="h-4" />

      <Tooltip content={PROJECT.folder}>
        <Button variant="quiet" size="xs" className="font-medium">
          {PROJECT.name}
        </Button>
      </Tooltip>

      <div className="ml-auto">
        <BuildPill />
      </div>
    </footer>
  );
}

function BuildPill() {
  const { buildState } = useAppState();
  if (buildState === "building") {
    return (
      <Badge variant="agent" size="md" icon={<Loader2 className="animate-spin" />} className="animate-pulse-subtle">
        Building rhythm-loops…
      </Badge>
    );
  }
  if (buildState === "failed") {
    return (
      <Tooltip content="rhythm-loops failed to compile. The last good build keeps running." side="top">
        <Badge variant="red" size="md" icon={<TriangleAlert />} tabIndex={0}>
          Build failed · running previous build
        </Badge>
      </Tooltip>
    );
  }
  return (
    <Tooltip content="All extensions built and loaded" side="top">
      <Badge variant="ghost" size="md" icon={<Hammer />} tabIndex={0} className="gap-1.5 text-gray-950/50">
        rhythm-loops <span className="tabular">2.2 s</span>
        <Check className="text-green-500" />
      </Badge>
    </Tooltip>
  );
}

function SeekStrip() {
  const { transport, setTransport } = useAppState();
  const ref = React.useRef<HTMLDivElement>(null);
  const pct = Math.min(100, (transport.position / LOOP_LENGTH) * 100);
  const seek = (e: React.PointerEvent) => {
    const r = ref.current?.getBoundingClientRect();
    if (!r) return;
    const t = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
    setTransport((s) => ({ ...s, position: t * LOOP_LENGTH }));
  };
  return (
    <div
      ref={ref}
      role="slider"
      tabIndex={0}
      aria-label="Seek"
      aria-valuemin={0}
      aria-valuemax={LOOP_LENGTH}
      aria-valuenow={Math.round(transport.position)}
      aria-valuetext={formatPosition(transport.position)}
      onPointerDown={(e) => {
        (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
        seek(e);
      }}
      onPointerMove={(e) => e.buttons === 1 && seek(e)}
      onKeyDown={(e) => {
        if (e.key === "ArrowRight") setTransport((s) => ({ ...s, position: Math.min(LOOP_LENGTH, s.position + 1) }));
        if (e.key === "ArrowLeft") setTransport((s) => ({ ...s, position: Math.max(0, s.position - 1) }));
        if (e.key === "Home") setTransport((s) => ({ ...s, position: 0 }));
      }}
      className="group/seek relative flex h-7 min-w-24 flex-1 cursor-ew-resize items-center rounded-md"
    >
      <div className="relative h-1 w-full overflow-visible rounded-full bg-alpha/10">
        <div className="absolute inset-y-0 left-0 rounded-full bg-green-500/70" style={{ width: `${pct}%` }} />
        {/* minute marks */}
        {[1, 2, 3].map((m) => (
          <span key={m} aria-hidden className="absolute -top-0.5 h-2 w-px bg-alpha/15" style={{ left: `${(m * 60 / LOOP_LENGTH) * 100}%` }} />
        ))}
        <div
          aria-hidden
          className="absolute top-1/2 size-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-gray-950 opacity-0 shadow-[0_0_0_2px_var(--color-gray-100)] transition-opacity duration-100 group-hover/seek:opacity-100 group-focus-visible/seek:opacity-100"
          style={{ left: `${pct}%` }}
        />
      </div>
    </div>
  );
}
