import type { Node } from "@xyflow/react";

import type { Activity, CanvasNode } from "./api";

export type ActivityNodeData = {
  provider: string;
  prompt: string;
  sessionId: string;
};

export function toCanvasNodes(
  activities: Activity[],
  canvasNodes: CanvasNode[],
): Node<ActivityNodeData>[] {
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
      position: { x: canvasNode.position_x, y: canvasNode.position_y },
      data: {
        provider: activity.provider,
        prompt: activity.prompt,
        sessionId: activity.session_id,
      },
    }];
  });
}
