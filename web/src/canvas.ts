import type { Edge } from "@xyflow/react";

import type { ActivitySummary, CanvasEdge, CanvasNode } from "./api";
import type { ActivityFlowNode } from "./components/ActivityNode";

export type ActivityNodeData = {
  activityId: number;
  project: ActivitySummary["project"];
  provider: string;
  activityKind: ActivitySummary["activity_kind"];
  prompt: string;
  time: ActivitySummary["time"];
  conversationIndex: number;
  conversationTotal: number;
  resultSummaryStatus: ActivitySummary["result_summary_status"];
  promptSummary: ActivitySummary["prompt_summary"];
};

export function toCanvasNodes(
  activities: ActivitySummary[],
  canvasNodes: CanvasNode[],
): ActivityFlowNode[] {
  const canvasByActivity = new Map(
    canvasNodes.map((node) => [node.activity_event_id, node]),
  );
  return activities.flatMap((activity) => {
    const canvasNode = canvasByActivity.get(activity.id);
    if (!canvasNode) {
      return [];
    }
    return [{
      id: `activity-${activity.id}`,
      type: "activity",
      position: { x: canvasNode.position_x, y: canvasNode.position_y },
      data: {
        activityId: activity.id,
        project: activity.project,
        provider: activity.provider,
        activityKind: activity.activity_kind,
        prompt: activity.prompt_summary.text ?? activity.prompt,
        time: activity.time,
        conversationIndex: activity.conversation_index,
        conversationTotal: activity.conversation_total,
        resultSummaryStatus: activity.result_summary_status,
        promptSummary: activity.prompt_summary,
      },
    }];
  });
}

export function toVisibleEdges(
  activities: ActivitySummary[],
  canvasNodes: CanvasNode[],
  persistedEdges: CanvasEdge[],
): Edge[] {
  const visibleActivityIds = new Set(activities.map(({ id }) => id));
  const idByCanvasNode = new Map(
    canvasNodes
      .filter(({ activity_event_id }) => visibleActivityIds.has(activity_event_id))
      .map(({ id, activity_event_id }) => [id, `activity-${activity_event_id}`]),
  );
  return persistedEdges.flatMap((edge) => {
    const source = idByCanvasNode.get(edge.source_node_id);
    const target = idByCanvasNode.get(edge.target_node_id);
    return source && target ? [{ id: `edge-${edge.id}`, source, target }] : [];
  });
}
