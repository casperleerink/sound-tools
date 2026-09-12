import * as React from "react";
import { getTool } from "@/extensions";
import { ToolFrame } from "@/ui";

/** #/ext/<type>: every view of one tool, each in its own frame, on the canvas grid. */
export function ExtensionPreview({ type }: { type: string }) {
  const tool = getTool(type);
  return (
    <div className="flex bg-gray-100 h-full flex-wrap items-start gap-12 overflow-auto p-12">
      {tool.views.map((view) => (
        <PreviewCard key={view.id} type={type} viewId={view.id} />
      ))}
    </div>
  );
}

function PreviewCard({ type, viewId }: { type: string; viewId: string }) {
  const tool = getTool(type);
  const [active, setActive] = React.useState(viewId);
  const view = tool.views.find((v) => v.id === active) ?? tool.views[0]!;
  const View = view.component;
  return (
    <ToolFrame
      instanceName={`${tool.name} 1`}
      typeName={tool.name}
      accent={tool.accent}
      views={tool.views}
      activeView={view.id}
      onViewChange={setActive}
      inputs={tool.inputs}
      outputs={tool.outputs}
      connectedPorts={new Set([...tool.inputs, ...tool.outputs].map((p) => p.id).filter((_, i) => i % 2 === 0))}
      width={view.width}
    >
      <View instanceId={`${type}-1`} instanceName={`${tool.name} 1`} accent={tool.accent} />
    </ToolFrame>
  );
}
