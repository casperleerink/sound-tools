import {
  ArrowUp,
  Check,
  ChevronDown,
  ChevronRight,
  FileCode2,
  FileJson2,
  FilePlus2,
  Hammer,
  History,
  Link2,
  Loader2,
  PanelRightClose,
  PanelRightOpen,
  Paperclip,
  RotateCcw,
  SquarePen,
  X,
} from "lucide-react";
import * as React from "react";
import { useAppState } from "@/app/app-state";
import { CONVERSATIONS, type Block, type Message, type ToolCall } from "@/data/conversation";
import { Button, cn, Dot, IconButton, KbdShortcut, Select, Tooltip } from "@/ui";

const MIN_W = 300;
const MAX_W = 560;

export function AgentSidebar() {
  const { sidebarOpen, setSidebarOpen, buildState } = useAppState();
  const [width, setWidth] = React.useState(() => (window.innerWidth < 1300 ? 320 : 372));
  const dragging = React.useRef<{ x: number; w: number } | null>(null);
  const messages = CONVERSATIONS[buildState];
  const scrollRef = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [buildState]);

  if (!sidebarOpen) {
    return (
      <aside aria-label="Agent" className="flex w-10 shrink-0 flex-col items-center gap-2 border-l border-alpha/5 bg-gray-100 py-2">
        <Tooltip content="Open agent" side="left">
          <IconButton label="Open agent sidebar" size="sm" variant="quiet" onClick={() => setSidebarOpen(true)}>
            <PanelRightOpen />
          </IconButton>
        </Tooltip>
        <Dot color="var(--color-lavender-500)" pulse={buildState !== "idle"} className="mt-1" />
      </aside>
    );
  }

  return (
    <aside aria-label="Agent" className="relative flex shrink-0 flex-col border-l border-alpha/5 bg-gray-100" style={{ width }}>
      {/* resize handle */}
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize agent sidebar"
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key === "ArrowLeft") setWidth((w) => Math.min(MAX_W, w + 16));
          if (e.key === "ArrowRight") setWidth((w) => Math.max(MIN_W, w - 16));
        }}
        onPointerDown={(e) => {
          dragging.current = { x: e.clientX, w: width };
          (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
        }}
        onPointerMove={(e) => {
          if (!dragging.current) return;
          setWidth(Math.max(MIN_W, Math.min(MAX_W, dragging.current.w - (e.clientX - dragging.current.x))));
        }}
        onPointerUp={() => (dragging.current = null)}
        className="absolute inset-y-0 -left-1 z-10 w-2 cursor-col-resize hover:bg-lavender-500/20 focus-visible:bg-lavender-500/30"
      />

      <header className="flex h-9 shrink-0 items-center gap-2 border-b border-alpha/5 pl-3 pr-1.5">
        <Dot color="var(--color-lavender-500)" pulse={buildState !== "idle"} />
        <h2 className="text-sm font-medium">Agent</h2>
        <span className="truncate text-xs text-gray-950/40">three-voices · 3 turns</span>
        <div className="ml-auto flex items-center gap-0.5">
          <Tooltip content="Session history">
            <IconButton label="Session history" size="sm" variant="quiet">
              <History />
            </IconButton>
          </Tooltip>
          <Tooltip content="New session">
            <IconButton label="New session" size="sm" variant="quiet">
              <SquarePen />
            </IconButton>
          </Tooltip>
          <Tooltip content="Collapse">
            <IconButton label="Collapse agent sidebar" size="sm" variant="quiet" onClick={() => setSidebarOpen(false)}>
              <PanelRightClose />
            </IconButton>
          </Tooltip>
        </div>
      </header>

      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
        <div className="flex flex-col gap-4">
          {messages.map((m) => (
            <MessageView key={m.id} message={m} />
          ))}
        </div>
      </div>

      <Composer />
    </aside>
  );
}

function MessageView({ message }: { message: Message }) {
  if (message.role === "user") {
    return (
      <div className="flex flex-col items-end gap-1">
        <div className="max-w-[92%] rounded-lg rounded-tr-sm bg-lavender-500/10 px-3 py-2 text-sm leading-5 text-gray-950">{message.text}</div>
        <span className="pr-1 text-2xs text-gray-950/30 tabular">{message.time}</span>
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-2.5">
      {message.blocks.map((b, i) => (
        <BlockView key={i} block={b} />
      ))}
      {message.working && (
        <div className="flex items-center gap-2 pl-0.5 text-xs text-lavender-500 animate-pulse-subtle">
          <Dot color="var(--color-lavender-500)" pulse />
          Working…
        </div>
      )}
    </div>
  );
}

function BlockView({ block }: { block: Block }) {
  if (block.kind === "text") return <p className="text-sm leading-5 text-gray-950/90">{block.text}</p>;
  if (block.kind === "status") {
    const styles = {
      ok: "bg-green-500/10 text-green-500",
      warn: "bg-orange-500/10 text-orange-500",
      error: "bg-red-500/10 text-red-500",
      info: "bg-lavender-500/10 text-lavender-500",
    }[block.tone];
    return <div className={cn("rounded-md px-2.5 py-1.5 text-xs font-medium leading-4", styles)}>{block.text}</div>;
  }
  return (
    <div className="overflow-hidden rounded-lg border border-alpha/5 bg-gray-200/60">
      {block.calls.map((c, i) => (
        <ToolCallRow key={i} call={c} first={i === 0} />
      ))}
    </div>
  );
}

/** Show the file and its parent folder only; the full path goes in the title. */
function pathParts(path: string) {
  const parts = path.split("/");
  const file = parts.pop() ?? path;
  const parent = parts.pop();
  return { dir: parent ? `${parent}/` : "", file };
}

function ToolCallRow({ call, first }: { call: ToolCall; first: boolean }) {
  const expandable = call.kind === "build";
  const [open, setOpen] = React.useState(call.kind === "build" && call.status !== "ok");

  let icon: React.ReactNode;
  let verb: string;
  let main: React.ReactNode;
  let right: React.ReactNode = null;

  const lines = (l?: { added: number; removed: number }) =>
    l && (
      <span className="text-2xs tabular">
        {l.added > 0 && <span className="text-green-500">+{l.added}</span>}
        {l.added > 0 && l.removed > 0 && " "}
        {l.removed > 0 && <span className="text-red-500">−{l.removed}</span>}
      </span>
    );

  const Path = ({ path }: { path: string }) => {
    const { dir, file } = pathParts(path);
    return (
      <span className="shrink-0 font-mono text-xs" title={path}>
        <span className="text-gray-950/40">{dir}</span>
        <span className="text-gray-950/90">{file}</span>
      </span>
    );
  };

  switch (call.kind) {
    case "read":
      icon = <FileJson2 />;
      verb = "Read";
      main = <Path path={call.path} />;
      break;
    case "edit":
      icon = call.path.endsWith(".rs") ? <FileCode2 /> : <FileJson2 />;
      verb = "Edit";
      main = (
        <span className="flex min-w-0 items-center gap-2">
          <Path path={call.path} />
          <span className="truncate text-xs text-gray-950/40">{call.summary}</span>
        </span>
      );
      right = lines(call.lines);
      break;
    case "create":
      icon = <FilePlus2 />;
      verb = "Create";
      main = (
        <span className="flex min-w-0 items-center gap-2">
          <Path path={call.path} />
          <span className="truncate text-xs text-gray-950/40">{call.summary}</span>
        </span>
      );
      right = lines(call.lines);
      break;
    case "connect":
      icon = <Link2 />;
      verb = "Connect";
      main = <span className="truncate text-xs text-gray-950/70">{call.summary}</span>;
      break;
    case "reload":
      icon = <RotateCcw />;
      verb = "Reload";
      main = <span className="truncate text-xs text-gray-950/70">{call.summary}</span>;
      right = <Check className="size-3.5 text-green-500" />;
      break;
    case "build":
      icon = call.status === "running" ? <Loader2 className="animate-spin text-lavender-500" /> : <Hammer />;
      verb = "Build";
      main = (
        <span className="flex min-w-0 items-center gap-2">
          <span className="truncate font-mono text-xs text-gray-950/90">{call.extension}</span>
          {call.status === "running" && <span className="text-xs text-lavender-500">building…</span>}
          {call.status === "failed" && <span className="text-xs text-red-500">failed</span>}
        </span>
      );
      right =
        call.status === "ok" ? (
          <span className="flex items-center gap-1.5 text-2xs text-gray-950/50 tabular">
            {call.seconds?.toFixed(1)} s <Check className="size-3.5 text-green-500" />
          </span>
        ) : call.status === "failed" ? (
          <span className="flex items-center gap-1.5 text-2xs text-gray-950/50 tabular">
            {call.seconds?.toFixed(1)} s <X className="size-3.5 text-red-500" />
          </span>
        ) : (
          <span className="text-2xs text-gray-950/50 tabular">1.4 s</span>
        );
      break;
  }

  const Row = expandable ? "button" : "div";

  return (
    <div className={cn(!first && "border-t border-alpha/5")}>
      <Row
        type={expandable ? "button" : undefined}
        aria-expanded={expandable ? open : undefined}
        onClick={expandable ? () => setOpen((v) => !v) : undefined}
        className={cn("flex h-7 w-full min-w-0 items-center gap-2 pl-2 pr-2 text-left", expandable && "hover:bg-alpha/[0.03]")}
      >
        <span className="flex size-4 shrink-0 items-center justify-center text-gray-950/50 [&_svg]:size-3.5">{icon}</span>
        <span className="w-11 shrink-0 text-xs font-medium text-gray-950/60">{verb}</span>
        <span className="flex min-w-0 flex-1 items-center">{main}</span>
        {right && <span className="shrink-0">{right}</span>}
        {expandable && (
          <span className="shrink-0 text-gray-950/40">{open ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}</span>
        )}
      </Row>
      {expandable && open && call.kind === "build" && (
        <pre className="max-h-48 overflow-auto border-t border-alpha/5 bg-gray-50/60 px-3 py-2 font-mono text-2xs leading-4 text-gray-950/70 whitespace-pre">
          {call.output.map((l, i) => (
            <div key={i} className={cn(l.startsWith("error") && "text-red-500", l.startsWith("help") && "text-blue-500", l.includes("Finished") && "text-green-500")}>
              {l}
            </div>
          ))}
          {call.status === "running" && <div className="text-lavender-500 animate-pulse-subtle">   Compiling three-voices-runtime v0.1.0 (.runtime)</div>}
        </pre>
      )}
    </div>
  );
}

const MODELS = [
  { value: "opus-5", label: "Claude Opus 5" },
  { value: "gpt-5.6", label: "GPT-5.6 Sol" },
  { value: "fable", label: "Claude Fable 5.1" },
];

function Composer() {
  const [model, setModel] = React.useState("opus-5");
  const [text, setText] = React.useState("");
  return (
    <div className="shrink-0 border-t border-alpha/5 p-2">
      <div className="rounded-lg border border-alpha/10 bg-gray-200 transition-colors duration-100 focus-within:border-lavender-500/60">
        <textarea
          aria-label="Message the agent"
          placeholder="Ask for a tool or a change…"
          rows={2}
          value={text}
          onChange={(e) => setText(e.target.value)}
          className="block w-full resize-none bg-transparent px-3 pt-2.5 pb-1 text-sm leading-5 text-gray-950 placeholder:text-gray-950/35 focus:outline-none"
        />
        <div className="flex items-center gap-1 px-1.5 pb-1.5">
          <Select label="Model" size="xs" value={model} onChange={setModel} options={MODELS} className="max-w-40 [&_select]:border-transparent [&_select]:bg-transparent [&_select]:hover:bg-alpha/5" />
          <span className="truncate text-2xs text-gray-950/35">Anthropic · subscription</span>
          <div className="ml-auto flex items-center gap-1">
            <Tooltip content="Attach a file">
              <IconButton label="Attach a file" size="xs" variant="quiet">
                <Paperclip />
              </IconButton>
            </Tooltip>
            <KbdShortcut shortcut="mod+enter" className="mr-0.5 opacity-70" />
            <Button variant="agent" size="icon-xs" aria-label="Send" disabled={text.trim().length === 0}>
              <ArrowUp />
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
