import { Pause, Play, Square } from "lucide-react";
import * as React from "react";
import { cn } from "@/lib/cn";
import { IconButton, Tooltip } from "@/ui";

/** Play/pause, stop, position, a quiet seek strip and the duration. */
export function TransportPill({ reloadPending }: { reloadPending: boolean }) {
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
      className="flex h-12 items-center gap-1 rounded-full border border-alpha/10 bg-gray-200/95 pr-5 pl-2 shadow-pill backdrop-blur-md"
    >
      <Tooltip content={playing ? "Pause" : "Play"} shortcut="space">
        <IconButton label={playing ? "Pause" : "Play"} size="md" variant={playing ? "green-subtle" : "ghost"} rounded onClick={() => setPlaying((p) => !p)}>
          {playing ? <Pause className="fill-current" /> : <Play className="fill-current" />}
        </IconButton>
      </Tooltip>
      <Tooltip content="Stop" shortcut="enter">
        <IconButton
          label="Stop"
          size="md"
          variant="ghost-muted"
          rounded
          onClick={() => {
            setPlaying(false);
            setPosition(0);
          }}
        >
          <Square className="size-3.5 fill-current" />
        </IconButton>
      </Tooltip>

      <div className="tabular ml-2 w-[84px] shrink-0 font-mono text-sm" aria-live="off" aria-label="Project position">
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
        className="focus-ring relative flex h-8 w-40 cursor-ew-resize items-center rounded-md"
      >
        <div className="relative h-0.5 w-full overflow-hidden rounded-full bg-alpha/10">
          <div className={cn("absolute inset-y-0 left-0", playing ? "bg-green-500" : "bg-gray-800")} style={{ width: `${(position / duration) * 100}%` }} />
        </div>
      </div>

      <span className="tabular ml-2 font-mono text-gray-700 text-xs">{formatTime(duration, false)}</span>

      {reloadPending ? (
        <Tooltip content="A reload is coming; playback will restart">
          <span role="status" aria-label="Reload pending" className="ml-3 flex size-4 items-center justify-center">
            <span className="size-1.5 animate-pulse-subtle rounded-full bg-lavender-500" />
          </span>
        </Tooltip>
      ) : null}
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
