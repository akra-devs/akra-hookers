import type { Node } from "@xyflow/react";

import type { Activity, CanvasNode } from "./api";

export type ActivityNodeData = {
  provider: string;
  prompt: string;
  sessionId: string;
};

export function toCanvasNodes(
  activities: Activity[],
  canvasNodes: CanvasNode[] = [],
): Node<ActivityNodeData>[] {
  return activities.map((activity, index) => ({
    id: `activity-${activity.id}`,
    position: positionFor(activity.id, index, canvasNodes),
    data: {
      provider: activity.provider,
      prompt: activity.prompt,
      sessionId: activity.session_id,
    },
  }));
}

function positionFor(activityId: number, index: number, canvasNodes: CanvasNode[]) {
  const canvasNode = canvasNodes.find((node) => node.activity_event_id === activityId);
  return canvasNode
    ? { x: canvasNode.position_x, y: canvasNode.position_y }
    : { x: 64 + (index % 3) * 280, y: 64 + Math.floor(index / 3) * 180 };
}
