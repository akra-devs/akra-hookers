import { MarkerType, type Edge } from "@xyflow/react";

import type { ActivitySummary, CanvasEdge, CanvasNode } from "./api";
import type { ActivityFlowNode } from "./components/ActivityNode";

const FILTERED_HORIZONTAL_GAP = 336;
const FILTERED_VERTICAL_GAP = 220;

export type CanvasPositionLayout = "persisted" | "compact-filtered";

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
  positionLayout: CanvasPositionLayout = "persisted",
): ActivityFlowNode[] {
  const canvasByActivity = new Map(
    canvasNodes.map((node) => [node.activity_event_id, node]),
  );
  const nodes: ActivityFlowNode[] = activities.flatMap((activity) => {
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
  return positionLayout === "compact-filtered"
    ? compactPositionGaps(nodes)
    : nodes;
}

function compactAxis(values: number[], maximumGap: number): Map<number, number> {
  const sorted = [...new Set(values.filter(Number.isFinite))].sort((a, b) => a - b);
  const compacted = new Map<number, number>();
  const first = sorted[0];
  if (first === undefined) return compacted;
  compacted.set(first, first);
  let previousSource = first;
  let previousCompacted = first;
  for (const value of sorted.slice(1)) {
    previousCompacted += Math.min(value - previousSource, maximumGap);
    compacted.set(value, previousCompacted);
    previousSource = value;
  }
  return compacted;
}

function compactPositionGaps(nodes: ActivityFlowNode[]): ActivityFlowNode[] {
  if (nodes.length < 2) return nodes;
  const compactedX = compactAxis(
    nodes.map(({ position }) => position.x),
    FILTERED_HORIZONTAL_GAP,
  );
  const compactedY = compactAxis(
    nodes.map(({ position }) => position.y),
    FILTERED_VERTICAL_GAP,
  );
  return nodes.map((node) => {
    const x = compactedX.get(node.position.x) ?? node.position.x;
    const y = compactedY.get(node.position.y) ?? node.position.y;
    return x === node.position.x && y === node.position.y
      ? node
      : { ...node, position: { x, y } };
  });
}

export function toVisibleEdges(
  activities: ActivitySummary[],
  canvasNodes: CanvasNode[],
  persistedEdges: CanvasEdge[],
): Edge[] {
  const visibleActivityIds = new Set(activities.map(({ id }) => id));
  const idByActivity = new Map(
    canvasNodes
      .filter(({ activity_event_id }) => visibleActivityIds.has(activity_event_id))
      .map(({ activity_event_id }) => [activity_event_id, `activity-${activity_event_id}`]),
  );
  const idByCanvasNode = new Map(
    canvasNodes
      .filter(({ activity_event_id }) => visibleActivityIds.has(activity_event_id))
      .map(({ id, activity_event_id }) => [id, `activity-${activity_event_id}`]),
  );
  const manualEdges = persistedEdges.flatMap((edge) => {
    const source = idByCanvasNode.get(edge.source_node_id);
    const target = idByCanvasNode.get(edge.target_node_id);
    return source && target ? [{ id: `edge-${edge.id}`, source, target }] : [];
  });
  const manualPairs = new Set(
    manualEdges.map(({ source, target }) => `${source}->${target}`),
  );
  const sequenceEdges: Edge[] = activities.flatMap((activity) => {
    const previousId = activity.previous_conversation_activity_id;
    if (previousId === null) return [];
    const source = idByActivity.get(previousId);
    const target = idByActivity.get(activity.id);
    if (!source || !target || manualPairs.has(`${source}->${target}`)) return [];
    return [{
      id: `sequence-${previousId}-${activity.id}`,
      source,
      target,
      type: "smoothstep",
      className: "activity-sequence-edge",
      selectable: false,
      deletable: false,
      focusable: false,
      ariaLabel: "요청 순서 연결",
      markerEnd: {
        type: MarkerType.ArrowClosed,
        color: "#8fc7a1",
        width: 14,
        height: 14,
      },
    }];
  });
  return [...sequenceEdges, ...manualEdges];
}
