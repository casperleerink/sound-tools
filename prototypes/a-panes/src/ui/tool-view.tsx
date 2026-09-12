import { ArrowDownLeft, ArrowUpRight, Layers2 } from "lucide-react";
import * as React from "react";
import { useViewContext, type ResolvedConnection } from "../sdk/view-context";
import { Badge } from "./badge";
import { cn } from "./cn";
import { Tooltip } from "./tooltip";

/**
 * Frame for one view of a tool instance: a slim header (name, meta, actions) and a body.
 * The pane sets --accent from the tool definition; everything inside inherits it.
 * The core adds the shared-instance marker, connections and rebuild state from ViewContext,
 * so extensions only pass title, meta, actions and children.
 */
export function ToolView({
  title,
  meta,
  actions,
  className,
  bodyClassName,
  children,
  accent,
  hideConnections,
}: {
  title: React.ReactNode;
  /** Small text right of the title (view name, short status). */
  meta?: React.ReactNode;
  actions?: React.ReactNode;
  className?: string;
  bodyClassName?: string;
  children: React.ReactNode;
  /** CSS colour; overrides inherited --accent. */
  accent?: string;
  hideConnections?: boolean;
}) {
  const ctx = useViewContext();
  const rebuilding = ctx?.rebuilding && ctx.rebuilding.extension === ctx.instance.extension ? ctx.rebuilding : null;
  const hasConnections = !hideConnections && ctx && (ctx.inputs.length > 0 || ctx.outputs.length > 0);

  return (
    <section
      className={cn("flex h-full min-h-0 min-w-0 flex-col bg-gray-200", className)}
      style={accent ? ({ "--accent": accent } as React.CSSProperties) : undefined}
    >
      <header className="flex h-9 shrink-0 items-center gap-2 border-b border-alpha/5 pl-3 pr-1.5">
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <h2 className="shrink-0 truncate text-sm font-medium text-gray-950">{title}</h2>
          {meta && <span className="min-w-0 truncate text-xs text-gray-950/40">{meta}</span>}
          {ctx && ctx.openViews > 1 && (
            <Tooltip content={`This instance is open in ${ctx.openViews} views. They edit the same state.`}>
              <Badge variant="accent" size="xs" icon={<Layers2 />} tabIndex={0} className="cursor-default">
                {ctx.openViews} views
              </Badge>
            </Tooltip>
          )}
          {rebuilding && (
            <Badge variant={rebuilding.failed ? "red" : "agent"} size="xs" className={rebuilding.failed ? "" : "animate-pulse-subtle"}>
              {rebuilding.failed ? "previous build" : "rebuilding"}
            </Badge>
          )}
        </div>
        {actions && <div className="flex shrink-0 items-center gap-0.5">{actions}</div>}
      </header>
      {hasConnections && <ConnectionsRow inputs={ctx.inputs} outputs={ctx.outputs} />}
      <div className={cn("relative min-h-0 min-w-0 flex-1 overflow-auto", bodyClassName)}>{children}</div>
    </section>
  );
}

const kindColor: Record<ResolvedConnection["kind"], string> = {
  audio: "var(--color-gray-800)",
  event: "var(--color-orange-500)",
  modulation: "var(--color-blue-500)",
};

function ConnectionsRow({ inputs, outputs }: { inputs: ResolvedConnection[]; outputs: ResolvedConnection[] }) {
  return (
    <div className="flex h-7 shrink-0 items-center gap-1 overflow-x-auto border-b border-alpha/5 px-2 scrollbar-hidden">
      {inputs.map((c) => (
        <Tooltip key={`in-${c.port}-${c.peer}`} content={`${c.kind} in · ${c.peer} ${c.peerPort} → ${c.port}`}>
          <button
            type="button"
            className="inline-flex h-5 shrink-0 items-center gap-1 rounded px-1.5 text-xs text-gray-950/60 transition-colors duration-100 hover:bg-alpha/5 hover:text-gray-950"
          >
            <ArrowDownLeft className="size-3" style={{ color: kindColor[c.kind] }} />
            <span className="font-medium">{c.peer}</span>
            <span className="text-gray-950/40">{c.peerPort}</span>
          </button>
        </Tooltip>
      ))}
      {inputs.length > 0 && outputs.length > 0 && <span className="mx-1 h-3 w-px shrink-0 bg-alpha/10" />}
      {outputs.map((c) => (
        <Tooltip key={`out-${c.port}-${c.peer}`} content={`${c.kind} out · ${c.port} → ${c.peer} ${c.peerPort}`}>
          <button
            type="button"
            className="inline-flex h-5 shrink-0 items-center gap-1 rounded px-1.5 text-xs text-gray-950/60 transition-colors duration-100 hover:bg-alpha/5 hover:text-gray-950"
          >
            <ArrowUpRight className="size-3" style={{ color: kindColor[c.kind] }} />
            <span className="font-medium">{c.peer}</span>
            <span className="text-gray-950/40">{c.peerPort}</span>
          </button>
        </Tooltip>
      ))}
    </div>
  );
}

/** Standard padded body. */
export function ToolViewBody({ className, children }: { className?: string; children: React.ReactNode }) {
  return <div className={cn("flex flex-col gap-5 p-4", className)}>{children}</div>;
}

/** Inline panel inside a view, for grouping (a voice strip, a settings box). */
export function Panel({ className, children, raised }: { className?: string; children: React.ReactNode; raised?: boolean }) {
  return (
    <div className={cn("rounded-lg border border-alpha/5", raised ? "bg-gray-300/40" : "bg-alpha/[0.03]", className)}>
      {children}
    </div>
  );
}
