import { ChevronDown, Plus, Redo2, Undo2 } from "lucide-react";
import { Button, IconButton, Separator, Tooltip } from "@/ui";
import { project } from "./project";

/** Top-left of the canvas: project name, undo/redo, add tool. */
export function ProjectCluster() {
  return (
    <div className="flex h-9 items-center gap-0.5 rounded-lg border border-alpha/10 bg-gray-200/95 p-0.5 shadow-card backdrop-blur-md">
      <Tooltip content={project.folder} side="bottom">
        <Button variant="ghost" size="sm" className="gap-1.5 pl-2.5 font-medium" aria-label={`Project ${project.name}`}>
          <span>{project.name}</span>
          <ChevronDown className="size-3.5 text-gray-700" />
        </Button>
      </Tooltip>
      <Separator orientation="vertical" className="mx-0.5 h-4" />
      <Tooltip content="Undo" shortcut="mod+z" side="bottom">
        <IconButton label="Undo" size="sm" variant="ghost-muted">
          <Undo2 />
        </IconButton>
      </Tooltip>
      <Tooltip content="Redo" shortcut="mod+shift+z" side="bottom">
        <IconButton label="Redo" size="sm" variant="ghost-muted" disabled>
          <Redo2 />
        </IconButton>
      </Tooltip>
      <Separator orientation="vertical" className="mx-0.5 h-4" />
      <Tooltip content="Add a tool to the canvas" shortcut="mod+k" side="bottom">
        <Button variant="subtle" size="sm" className="gap-1 pr-2.5">
          <Plus />
          <span>Add tool</span>
        </Button>
      </Tooltip>
    </div>
  );
}
