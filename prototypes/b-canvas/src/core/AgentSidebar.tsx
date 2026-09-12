import {
  ArrowUp,
  Check,
  ChevronDown,
  Code2,
  FileJson,
  FileText,
  Hammer,
  History,
  Loader2,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  RotateCw,
  Square,
  X,
} from "lucide-react";
import * as React from "react";
import { cn } from "@/lib/cn";
import { Badge, Button, IconButton, KbdShortcut, Tooltip } from "@/ui";
import type { Message, MessagePart, ToolCall, ToolCallKind } from "./conversation";
import type { DevState } from "./dev-state";

export interface AgentSidebarProps {
  messages: Message[];
  state: DevState;
  collapsed: boolean;
  onToggleCollapsed: () => void;
  /** Prototype-only chrome under the composer. */
  footer?: React.ReactNode;
}

export function AgentSidebar({ messages, state, collapsed, onToggleCollapsed, footer }: AgentSidebarProps) {
  const working = state !== "default";
  if (collapsed) {
    return (
      <aside aria-label="Agent" className="flex h-full w-12 shrink-0 flex-col items-center border-alpha/10 border-r bg-gray-50 pt-9">
        <Tooltip content="Open agent" shortcut="mod+j" side="right">
          <IconButton label="Open agent" variant="ghost-muted" onClick={onToggleCollapsed}>
            <PanelLeftOpen />
          </IconButton>
        </Tooltip>
        <span className={cn("mt-3 size-2 rounded-full bg-lavender-500", working && "animate-pulse-subtle")} aria-hidden />
      </aside>
    );
  }
  return (
    <aside aria-label="Agent" className="flex h-full w-[300px] shrink-0 flex-col border-alpha/10 border-r bg-gray-50">
      {/* Header. Left padding leaves room for macOS traffic lights. */}
      <div className="app-drag-region flex h-11 shrink-0 items-center gap-2 pr-2 pl-[76px]">
        <span className={cn("size-2 rounded-full bg-lavender-500", working && "animate-pulse-subtle")} aria-hidden />
        <span className="font-medium text-sm">Agent</span>
        <span className="flex-1" />
        <div className="app-no-drag flex items-center">
          <Tooltip content="New session">
            <IconButton label="New session" size="sm" variant="ghost-muted">
              <Plus />
            </IconButton>
          </Tooltip>
          <Tooltip content="History">
            <IconButton label="Session history" size="sm" variant="ghost-muted">
              <History />
            </IconButton>
          </Tooltip>
          <Tooltip content="Collapse" shortcut="mod+j">
            <IconButton label="Collapse agent sidebar" size="sm" variant="ghost-muted" onClick={onToggleCollapsed}>
              <PanelLeftClose />
            </IconButton>
          </Tooltip>
        </div>
      </div>

      <MessageList messages={messages} />

      <Composer working={working} footer={footer} />
    </aside>
  );
}

function MessageList({ messages }: { messages: Message[] }) {
  const ref = React.useRef<HTMLDivElement>(null);
  React.useEffect(() => {
    const el = ref.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages]);
  return (
      <div ref={ref} className="min-h-0 flex-1 overflow-y-auto px-3 pb-2 [scrollbar-gutter:stable]">
        <div className="mb-1.5 flex items-center gap-2 px-1 text-2xs text-gray-700">
          <span className="truncate">Session · Polyrhythm for three voices</span>
          <span className="tabular shrink-0">{messages[0]?.time}</span>
          <span className="h-px flex-1 bg-alpha/10" />
        </div>
        <div className="flex flex-col gap-3">
          {messages.map((m) => (
            <MessageView key={m.id} message={m} />
          ))}
        </div>
      </div>
  );
}

function MessageView({ message }: { message: Message }) {
  if (message.role === "user") {
    return (
      <div className="flex flex-col items-end gap-1">
        <div className="max-w-[92%] rounded-lg rounded-tr-sm bg-alpha/5 px-3 py-2 text-gray-950 text-sm leading-5" title={message.time}>
          {message.parts.map((p) => (p.type === "text" ? p.text : null))}
        </div>
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-2">
      {message.parts.map((part, i) => (
        <PartView key={i} part={part} />
      ))}
    </div>
  );
}

function PartView({ part }: { part: MessagePart }) {
  if (part.type === "text") return <p className="px-1 text-gray-950 text-sm leading-5">{part.text}</p>;
  if (part.type === "working")
    return (
      <div className="flex items-center gap-2 px-1 text-gray-800 text-xs">
        <Loader2 className="size-3.5 animate-spin-slow text-lavender-500" aria-hidden />
        <span className="animate-pulse-subtle">{part.text}</span>
      </div>
    );
  return (
    <div className="overflow-hidden rounded-lg border border-alpha/10 bg-gray-100">
      {part.calls.map((c, i) => (
        <ToolCallRow key={c.id} call={c} first={i === 0} />
      ))}
    </div>
  );
}

const kindIcon: Record<ToolCallKind, React.ComponentType<{ className?: string }>> = {
  read: FileText,
  "edit-record": FileJson,
  "edit-source": Code2,
  build: Hammer,
  reload: RotateCw,
};

function ToolCallRow({ call, first }: { call: ToolCall; first: boolean }) {
  const Icon = kindIcon[call.kind];
  const isBuild = call.kind === "build";
  const tone =
    call.status === "failed" ? "text-red-500" : call.status === "running" ? "text-lavender-500" : call.status === "pending" ? "text-gray-700" : "text-gray-800";
  return (
    <div className={cn("flex flex-col", !first && "border-alpha/5 border-t", call.status === "pending" && "opacity-60")}>
      <div className="flex h-8 items-center gap-2 px-2.5">
        <Icon className={cn("size-3.5 shrink-0", isBuild && call.status !== "done" ? tone : "text-gray-700")} />
        <span className={cn("shrink-0 font-medium text-xs", call.status === "failed" ? "text-red-500" : call.status === "running" && isBuild ? "text-lavender-500" : "text-gray-950")}>
          {call.label}
        </span>
        <span className="min-w-0 flex-1 truncate font-mono text-2xs text-gray-800" title={call.target}>
          {shortPath(call.target)}
        </span>
        {call.meta ? <span className="tabular shrink-0 text-2xs text-gray-700">{call.meta}</span> : null}
        <StatusIcon status={call.status} />
      </div>
      {call.status === "running" && call.progress !== undefined ? (
        <div className="px-2.5 pb-2">
          <div className="h-1 overflow-hidden rounded-full bg-alpha/10">
            <div className="h-full rounded-full bg-lavender-500 transition-[width] duration-300" style={{ width: `${call.progress * 100}%` }} />
          </div>
        </div>
      ) : null}
      {call.note ? (
        <div className={cn("flex items-center gap-1.5 px-2.5 pb-2 text-2xs", call.status === "failed" ? "text-orange-500" : "text-gray-700")}>
          {call.kind === "edit-record" ? <Badge variant="green" size="xs">no build</Badge> : null}
          <span>{call.note}</span>
        </div>
      ) : null}
      {call.output ? (
        <pre className="mx-2.5 mb-2.5 overflow-x-auto whitespace-pre rounded-md border border-red-500/20 bg-gray-50 p-2 font-mono text-2xs text-gray-900 leading-4 [&_b]:text-red-500">
          <BuildOutput text={call.output} />
        </pre>
      ) : null}
    </div>
  );
}

/** "extensions/step-seq/src/lib.rs" -> "src/lib.rs"; short targets stay as they are. */
function shortPath(target: string) {
  const parts = target.split("/");
  return parts.length > 2 ? parts.slice(-2).join("/") : target;
}

function BuildOutput({ text }: { text: string }) {
  return (
    <>
      {text.split("\n").map((line, i) => (
        <span key={i} className={cn("block", line.startsWith("error") && "text-red-500", line.startsWith("help") && "text-teal-500", /\^+/.test(line) && "text-red-500")}>
          {line}
        </span>
      ))}
    </>
  );
}

function StatusIcon({ status }: { status: ToolCall["status"] }) {
  if (status === "done") return <Check className="size-3.5 shrink-0 text-green-500" aria-label="done" />;
  if (status === "running") return <Loader2 className="size-3.5 shrink-0 animate-spin-slow text-lavender-500" aria-label="running" />;
  if (status === "failed") return <X className="size-3.5 shrink-0 text-red-500" aria-label="failed" />;
  return <span className="size-3.5 shrink-0" aria-label="pending" />;
}

function Composer({ working, footer }: { working: boolean; footer?: React.ReactNode }) {
  const [text, setText] = React.useState("");
  return (
    <div className="shrink-0 p-3 pt-0">
      <div className="flex flex-col rounded-[10px] border border-alpha/10 bg-gray-200 transition-colors duration-100 focus-within:border-lavender-500/50">
        <label htmlFor="composer" className="sr-only">
          Message the agent
        </label>
        <textarea
          id="composer"
          rows={1}
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="Ask for a tool, or a change to one…"
          className="max-h-40 min-h-10 w-full resize-none bg-transparent px-3 pt-2 text-gray-950 text-sm leading-5 outline-none placeholder:text-gray-700"
        />
        <div className="flex items-center gap-1 px-1.5 pb-1.5">
          <Button variant="ghost-muted" size="xs" className="gap-1 pr-1 text-gray-900" aria-label="Model: Claude Opus 5 via Anthropic">
            <span className="size-1.5 rounded-full bg-lavender-500" aria-hidden />
            <span>Opus 5</span>
            <span className="text-gray-700">Anthropic</span>
            <ChevronDown className="text-gray-700" />
          </Button>
          <Button variant="ghost-muted" size="xs" className="text-gray-800">
            Plan
          </Button>
          <span className="flex-1" />
          <Tooltip content="Send" side="top">
            <span className="flex items-center gap-1.5">
              <KbdShortcut shortcut="mod+enter" />
              {working ? (
                <IconButton label="Stop" size="xs" variant="subtle">
                  <Square className="size-3 fill-current" />
                </IconButton>
              ) : (
                <IconButton label="Send" size="xs" variant="agent" disabled={text.length === 0}>
                  <ArrowUp />
                </IconButton>
              )}
            </span>
          </Tooltip>
        </div>
      </div>
      {footer ? <div className="mt-1.5 px-1">{footer}</div> : null}
    </div>
  );
}
