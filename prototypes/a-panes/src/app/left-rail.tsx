import { Blocks, Boxes, ChevronDown, PanelLeftClose, PanelLeftOpen } from "lucide-react";
import * as React from "react";
import { INSTANCES } from "@/data/project";
import { EXTENSIONS, toolFor } from "@/extensions";
import { ACCENT_VAR } from "@/sdk/types";
import { Badge, cn, Dot, IconButton, Tooltip } from "@/ui";
import { openViewCount, useAppState } from "./app-state";

export function LeftRail() {
  const { railOpen, setRailOpen, layout, buildState } = useAppState();
  const [instancesOpen, setInstancesOpen] = React.useState(true);
  const [extensionsOpen, setExtensionsOpen] = React.useState(true);

  if (!railOpen) {
    return (
      <nav aria-label="Project" className="flex w-10 shrink-0 flex-col items-center gap-1 border-r border-alpha/5 bg-gray-100 py-2">
        <Tooltip content="Show project" side="right">
          <IconButton label="Show project rail" size="sm" variant="quiet" onClick={() => setRailOpen(true)}>
            <PanelLeftOpen />
          </IconButton>
        </Tooltip>
        <Tooltip content="Instances" side="right">
          <IconButton label="Instances" size="sm" variant="quiet" onClick={() => setRailOpen(true)}>
            <Boxes />
          </IconButton>
        </Tooltip>
        <Tooltip content="Extensions" side="right">
          <IconButton label="Extensions" size="sm" variant="quiet" onClick={() => setRailOpen(true)}>
            <Blocks />
          </IconButton>
        </Tooltip>
      </nav>
    );
  }

  return (
    <nav aria-label="Project" className="flex w-52 shrink-0 flex-col border-r border-alpha/5 bg-gray-100">
      <div className="flex h-9 shrink-0 items-center justify-between border-b border-alpha/5 pl-3 pr-1.5">
        <span className="text-sm font-medium">Project</span>
        <Tooltip content="Hide">
          <IconButton label="Hide project rail" size="sm" variant="quiet" onClick={() => setRailOpen(false)}>
            <PanelLeftClose />
          </IconButton>
        </Tooltip>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto py-1">
        <SectionHeader title="Instances" count={INSTANCES.length} open={instancesOpen} onToggle={() => setInstancesOpen((v) => !v)} />
        {instancesOpen &&
          INSTANCES.map((inst) => {
            const tool = toolFor(inst);
            const open = openViewCount(layout, inst.id);
            return (
              <button
                key={inst.id}
                type="button"
                className="flex h-7 w-full items-center gap-2 pl-4 pr-2 text-sm hover:bg-alpha/5"
              >
                <Dot color={ACCENT_VAR[tool.accent]} />
                <span className="truncate font-medium text-gray-950/90">{inst.name}</span>
                {tool.name !== inst.name && <span className="truncate text-xs text-gray-950/40">{tool.name}</span>}
                {open > 0 && <span className="ml-auto text-2xs text-gray-950/40 tabular">{open}</span>}
              </button>
            );
          })}
        <SectionHeader title="Extensions" count={EXTENSIONS.length} open={extensionsOpen} onToggle={() => setExtensionsOpen((v) => !v)} className="mt-2" />
        {extensionsOpen &&
          EXTENSIONS.map((ext) => {
            const editing = ext.name === "rhythm-loops";
            return (
              <button key={ext.name} type="button" className="flex h-7 w-full items-center gap-2 pl-4 pr-2 text-sm hover:bg-alpha/5">
                <span className="truncate font-mono text-xs text-gray-950/80">{ext.name}</span>
                {editing && buildState === "building" && (
                  <Badge variant="agent" size="xs" className="ml-auto animate-pulse-subtle">
                    building
                  </Badge>
                )}
                {editing && buildState === "failed" && (
                  <Badge variant="red" size="xs" className="ml-auto">
                    failed
                  </Badge>
                )}
                {editing && buildState === "idle" && <span className="ml-auto text-2xs text-gray-950/40 tabular">14:09</span>}
              </button>
            );
          })}
      </div>
    </nav>
  );
}

function SectionHeader({ title, count, open, onToggle, className }: { title: string; count: number; open: boolean; onToggle: () => void; className?: string }) {
  return (
    <button
      type="button"
      aria-expanded={open}
      onClick={onToggle}
      className={cn("flex h-7 w-full items-center gap-1 px-2 text-2xs font-semibold uppercase tracking-[0.08em] text-gray-950/40 hover:text-gray-950/70", className)}
    >
      <ChevronDown className={cn("size-3 transition-transform duration-100", !open && "-rotate-90")} />
      {title}
      <span className="ml-auto font-medium normal-case tracking-normal tabular">{count}</span>
    </button>
  );
}
