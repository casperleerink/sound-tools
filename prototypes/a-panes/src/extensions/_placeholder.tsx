import { ToolView, ToolViewBody } from "@/sdk";
import type { ToolViewProps } from "@/sdk";

/** Temporary view used while an extension is being written. */
export function PlaceholderView({ instance, viewId }: ToolViewProps) {
  return (
    <ToolView title={instance.name} meta={viewId}>
      <ToolViewBody>
        <p className="text-sm text-gray-950/50">View "{viewId}" for {instance.tool} is not written yet.</p>
      </ToolViewBody>
    </ToolView>
  );
}
