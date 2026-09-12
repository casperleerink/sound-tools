import { ArrowUp, ChevronDown, Plus, Square } from "lucide-react";
import * as React from "react";
import { cn } from "@/lib/cn";
import { Button, IconButton } from "@/ui";
import type { Step, Turn } from "./conversation";

export function AgentSidebar({ turns }: { turns: Turn[] }) {
  const working = turns.some((t) => t.activity.state !== "done");
  return (
    <aside aria-label="Agent" className="flex h-full w-[340px] shrink-0 flex-col border-alpha/10 border-r bg-gray-50">
      {/* Header. Left padding leaves room for macOS traffic lights. */}
      <div className="app-drag-region flex h-12 shrink-0 items-center pr-4 pl-[76px]">
        <span className="font-medium text-sm">Agent</span>
        <span className="flex-1" />
        <IconButton label="New session" size="sm" variant="ghost-muted" className="app-no-drag">
          <Plus />
        </IconButton>
      </div>

      <TurnList turns={turns} />

      <Composer working={working} />
    </aside>
  );
}

function TurnList({ turns }: { turns: Turn[] }) {
  const ref = React.useRef<HTMLDivElement>(null);
  React.useEffect(() => {
    const el = ref.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [turns]);
  return (
    <div ref={ref} className="min-h-0 flex-1 overflow-y-auto px-6 pt-4 pb-6 [scrollbar-gutter:stable]">
      <div className="flex flex-col gap-8">
        {turns.map((t) => (
          <TurnView key={t.id} turn={t} />
        ))}
      </div>
    </div>
  );
}

function TurnView({ turn }: { turn: Turn }) {
  return (
    <div className="flex flex-col gap-4">
      <p className="self-end rounded-lg rounded-tr-sm bg-alpha/5 px-4 py-3 text-md">{turn.request}</p>
      <ActivityLine turn={turn} />
      {turn.result ? <p className="text-md">{turn.result}</p> : null}
    </div>
  );
}

/** One line: pulsing while working, red when the build failed, "Worked for 12 s" after. Click to see the steps. */
function ActivityLine({ turn }: { turn: Turn }) {
  const [open, setOpen] = React.useState(false);
  const a = turn.activity;
  const label = a.state === "done" ? `Worked for ${a.seconds} s` : a.label;
  return (
    <div className="flex flex-col gap-2">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className={cn(
          "focus-ring -ml-2 flex h-7 w-fit items-center gap-2 rounded-md px-2 text-sm transition-colors duration-100 hover:bg-alpha/5",
          a.state === "working" && "animate-pulse-subtle text-gray-950",
          a.state === "failed" && "text-red-500",
          a.state === "done" && "text-gray-700 hover:text-gray-900",
        )}
      >
        {a.state === "working" ? <span aria-hidden className="size-1.5 rounded-full bg-lavender-500" /> : null}
        {a.state === "failed" ? <span aria-hidden className="size-1.5 rounded-full bg-red-500" /> : null}
        {label}
      </button>
      {open ? <History steps={turn.history} /> : null}
    </div>
  );
}

function History({ steps }: { steps: Step[] }) {
  return (
    <ol className="flex flex-col gap-1.5 border-alpha/10 border-l pl-3 text-xs">
      {steps.map((s, i) => (
        <li key={i} className="flex items-baseline gap-2 text-gray-800">
          <span className="w-10 shrink-0 text-gray-700">{s.label}</span>
          <span className="min-w-0 truncate font-mono" title={s.target}>
            {shortPath(s.target)}
          </span>
          {s.note ? <span className="tabular ml-auto shrink-0 whitespace-nowrap text-gray-700">{s.note}</span> : null}
        </li>
      ))}
    </ol>
  );
}

/** "extensions/step-seq/src/lib.rs" -> "src/lib.rs"; short targets stay as they are. */
function shortPath(target: string) {
  const parts = target.split("/");
  return parts.length > 2 ? parts.slice(-2).join("/") : target;
}

function Composer({ working }: { working: boolean }) {
  const [text, setText] = React.useState("");
  return (
    <div className="shrink-0 px-6 pb-6">
      <div className="flex flex-col rounded-[10px] border border-alpha/10 bg-gray-200 transition-colors duration-100 focus-within:border-lavender-500/50">
        <label htmlFor="composer" className="sr-only">
          Message the agent
        </label>
        <textarea
          id="composer"
          rows={2}
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="Ask for a tool, or a change to one"
          className="max-h-40 w-full resize-none bg-transparent px-4 pt-3 text-md outline-none placeholder:text-gray-700"
        />
        <div className="flex items-center px-2 pb-2">
          <Button variant="ghost-muted" size="sm" className="gap-1 pr-1.5 pl-2 text-gray-800" aria-label="Model: Opus 5">
            Opus 5
            <ChevronDown className="size-3.5" />
          </Button>
          <span className="flex-1" />
          {working ? (
            <IconButton label="Stop" size="sm" variant="subtle" rounded>
              <Square className="size-3 fill-current" />
            </IconButton>
          ) : (
            <IconButton label="Send" size="sm" variant="agent" rounded disabled={text.length === 0}>
              <ArrowUp />
            </IconButton>
          )}
        </div>
      </div>
    </div>
  );
}
