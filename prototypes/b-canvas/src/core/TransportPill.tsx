import { Check, ChevronDown, Hammer, Loader2, Pause, Play, Speaker, Square, TriangleAlert } from "lucide-react";
import * as React from "react";
import { cn } from "@/lib/cn";
import { IconButton, Separator, Tooltip } from "@/ui";
import type { BuildStatus } from "./conversation";
import { project } from "./project";

export function TransportPill({ build }: { build: BuildStatus }) {
  const [playing, setPlaying] = React.useState(true);
  const [position, setPosition] = React.useState(84.36); // seconds
  const duration = 200;
  const seekRef = React.useRef<HTMLDivElement>(null);

  const seek = (e: React.PointerEvent) => {
    const r = seekRef.current?.getBoundingClientRect();
    if (!r) return;
    const t = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
    setPosition(t * duration);
  };

  return (
    <div
      role="toolbar"
      aria-label="Transport"
      className="flex h-12 items-center gap-1 rounded-full border border-alpha/10 bg-gray-200/95 pr-2 pl-2 shadow-pill backdrop-blur-md"
    >
      <Tooltip content={playing ? "Pause" : "Play"} shortcut="space">
        <IconButton
          label={playing ? "Pause" : "Play"}
          size="md"
          variant={playing ? "green-subtle" : "ghost"}
          rounded
          onClick={() => setPlaying((p) => !p)}
        >
          {playing ? <Pause className="fill-current" /> : <Play className="fill-current" />}
        </IconButton>
      </Tooltip>
      <Tooltip content="Stop" shortcut="enter">
        <IconButton label="Stop" size="md" variant="ghost" rounded onClick={() => { setPlaying(false); setPosition(0); }}>
          <Square className="size-3.5 fill-current" />
        </IconButton>
      </Tooltip>

      <div className="tabular ml-1 w-[92px] shrink-0 text-center font-mono text-gray-950 text-sm" aria-live="off" aria-label="Project position">
        {formatTime(position)}
      </div>

      <div
        ref={seekRef}
        role="slider"
        aria-label="Seek"
        aria-valuemin={0}
        aria-valuemax={duration}
        aria-valuenow={Math.round(position)}
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key === "ArrowLeft") setPosition((p) => Math.max(0, p - 5));
          if (e.key === "ArrowRight") setPosition((p) => Math.min(duration, p + 5));
        }}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
          seek(e);
        }}
        onPointerMove={(e) => {
          if (e.currentTarget.hasPointerCapture(e.pointerId)) seek(e);
        }}
        className="focus-ring group/seek relative mx-1 flex h-8 w-44 cursor-ew-resize items-center rounded-md"
      >
        <div className="relative h-1 w-full overflow-hidden rounded-full bg-alpha/10">
          {/* cycle markers every 12 s: the polyrhythm repeat */}
          {Array.from({ length: Math.floor(duration / 12) }, (_, i) => (
            <span key={i} className="absolute top-0 bottom-0 w-px bg-alpha/15" style={{ left: `${((i + 1) * 12 * 100) / duration}%` }} />
          ))}
          <div className={cn("absolute inset-y-0 left-0 rounded-full", playing ? "bg-green-500" : "bg-gray-800")} style={{ width: `${(position / duration) * 100}%` }} />
        </div>
        <div
          className={cn("absolute top-1/2 h-3 w-0.5 -translate-y-1/2 rounded-full transition-transform group-hover/seek:scale-y-125", playing ? "bg-green-500" : "bg-gray-950")}
          style={{ left: `calc(${(position / duration) * 100}% - 1px)` }}
        />
      </div>
      <span className="tabular mr-1 font-mono text-gray-700 text-xs">{formatTime(duration, false)}</span>

      <Separator orientation="vertical" className="mx-1 h-5" />

      <BuildChip build={build} />

      <Separator orientation="vertical" className="mx-1 h-5" />

      <Tooltip content="Audio output device">
        <button
          type="button"
          className="focus-ring flex h-8 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-full px-2 text-gray-900 text-xs transition-colors duration-100 hover:bg-alpha/5 hover:text-gray-950"
          aria-label={`Audio device: ${project.device}, ${project.sampleRate / 1000} kHz`}
        >
          <Speaker className="size-3.5 text-gray-700" />
          <span>{project.device}</span>
          <span className="tabular text-gray-700">{project.sampleRate / 1000} kHz</span>
          <ChevronDown className="size-3 text-gray-700" />
        </button>
      </Tooltip>
    </div>
  );
}

function BuildChip({ build }: { build: BuildStatus }) {
  const base = "flex h-8 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-full px-2.5 text-xs";
  if (build.state === "building")
    return (
      <div className={cn(base, "text-orange-500")} role="status">
        <Loader2 className="size-3.5 animate-spin-slow" />
        <span className="font-medium">{build.label}</span>
        <div className="h-1 w-16 overflow-hidden rounded-full bg-alpha/10">
          <div className="h-full rounded-full bg-orange-500" style={{ width: `${(build.progress ?? 0) * 100}%` }} />
        </div>
        <span className="tabular text-gray-700">{build.detail}</span>
      </div>
    );
  if (build.state === "failed")
    return (
      <div className={cn(base, "text-red-500")} role="status">
        <TriangleAlert className="size-3.5" />
        <span className="font-medium">{build.label}</span>
        <span className="text-gray-700">· {build.detail}</span>
      </div>
    );
  return (
    <div className={cn(base, "text-gray-900")} role="status">
      {build.state === "reloaded" ? <Check className="size-3.5 text-green-500" /> : <Hammer className="size-3.5 text-gray-700" />}
      <span className="font-medium">{build.label}</span>
      {build.detail ? <span className="tabular text-gray-700">· {build.detail}</span> : null}
    </div>
  );
}

export function formatTime(seconds: number, withCentis = true) {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  const c = Math.floor((seconds % 1) * 100);
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  return withCentis ? `${mm}:${ss}.${String(c).padStart(2, "0")}` : `${mm}:${ss}`;
}
