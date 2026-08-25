import { MarkerType } from "@xyflow/react";
import { describe, expect, it } from "vitest";

import type { ActivitySummary } from "./api";
import { toCanvasNodes, toVisibleEdges } from "./canvas";

function activity(
  id: number,
  previousConversationActivityId: number | null = null,
): ActivitySummary {
  return {
    id,
    provider: "codex",
    activity_kind: "user",
    prompt: `request ${id}`,
    project: null,
    time: { value: null, provenance: "unknown" },
    previous_conversation_activity_id: previousConversationActivityId,
    conversation_index: id,
    conversation_total: 3,
    result_summary_status: "unavailable",
    prompt_summary: { status: "unavailable", mode: "fallback", text: null },
  };
}

describe("toCanvasNodes", () => {
  it("creates a stable removable node per immutable activity", () => {
    const nodes = toCanvasNodes(
      [
        {
          id: 9,
          provider: "codex",
          activity_kind: "user",
          prompt: "Add a health endpoint",
          project: null,
          time: { value: null, provenance: "unknown" },
          previous_conversation_activity_id: null,
          conversation_index: 1,
          conversation_total: 1,
          result_summary_status: "ready",
          prompt_summary: { status: "ready", mode: "contextual", text: "Add a health endpoint" },
        },
      ],
      [{ id: 3, activity_event_id: 9, position_x: 64, position_y: 64 }],
    );

    expect(nodes).toMatchObject([
      {
        id: "activity-9",
        type: "activity",
        position: { x: 64, y: 64 },
        data: {
          activityId: 9,
          project: null,
          provider: "codex",
          prompt: "Add a health endpoint",
          time: { value: null, provenance: "unknown" },
          conversationIndex: 1,
          conversationTotal: 1,
          resultSummaryStatus: "ready",
          promptSummary: { status: "ready", mode: "contextual", text: "Add a health endpoint" },
        },
      },
    ]);
    expect(Object.keys(nodes[0]?.data ?? {}).sort()).toEqual([
      "activityId",
      "activityKind",
      "conversationIndex",
      "conversationTotal",
      "project",
      "prompt",
      "promptSummary",
      "provider",
      "resultSummaryStatus",
      "time",
    ]);
  });

  it("uses durable canvas positions when supplied by the API", () => {
    const nodes = toCanvasNodes(
      [{
        id: 9,
        provider: "codex",
        activity_kind: "user",
        prompt: "keep",
        project: null,
        time: { value: null, provenance: "unknown" },
        previous_conversation_activity_id: null,
        conversation_index: 1,
        conversation_total: 1,
        result_summary_status: "unavailable",
        prompt_summary: { status: "unavailable", mode: "fallback", text: null },
      }],
      [{ id: 3, activity_event_id: 9, position_x: 120, position_y: 220 }],
    );

    expect(nodes[0]?.position).toEqual({ x: 120, y: 220 });
  });

  it("does not recreate an activity whose removable canvas node is absent", () => {
    const nodes = toCanvasNodes(
      [{
        id: 9,
        provider: "codex",
        activity_kind: "user",
        prompt: "removed",
        project: null,
        time: { value: null, provenance: "unknown" },
        previous_conversation_activity_id: null,
        conversation_index: 1,
        conversation_total: 1,
        result_summary_status: "unavailable",
        prompt_summary: { status: "unavailable", mode: "fallback", text: null },
      }],
      [],
    );

    expect(nodes).toEqual([]);
  });
});

describe("toVisibleEdges", () => {
  const canvasNodes = [
    { id: 11, activity_event_id: 1, position_x: 64, position_y: 64 },
    { id: 12, activity_event_id: 2, position_x: 400, position_y: 64 },
    { id: 13, activity_event_id: 3, position_x: 400, position_y: 284 },
  ];

  it("derives directional edges in conversation order", () => {
    const edges = toVisibleEdges(
      [activity(1), activity(2, 1), activity(3, 2)],
      canvasNodes,
      [],
    );

    expect(edges).toMatchObject([
      {
        id: "sequence-1-2",
        source: "activity-1",
        target: "activity-2",
        selectable: false,
        deletable: false,
        markerEnd: { type: MarkerType.ArrowClosed },
      },
      {
        id: "sequence-2-3",
        source: "activity-2",
        target: "activity-3",
        selectable: false,
        deletable: false,
        markerEnd: { type: MarkerType.ArrowClosed },
      },
    ]);
  });

  it("does not create dangling sequence edges", () => {
    expect(toVisibleEdges(
      [activity(2, 1)],
      canvasNodes.filter(({ activity_event_id }) => activity_event_id === 2),
      [],
    )).toEqual([]);
  });

  it("keeps a persisted edge instead of drawing the same pair twice", () => {
    expect(toVisibleEdges(
      [activity(1), activity(2, 1)],
      canvasNodes,
      [{ id: 21, source_node_id: 11, target_node_id: 12 }],
    )).toEqual([{ id: "edge-21", source: "activity-1", target: "activity-2" }]);
  });
});
