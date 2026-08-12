import { describe, expect, it } from "vitest";

import { toCanvasNodes } from "./canvas";

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
          conversation_index: 1,
          conversation_total: 1,
          result_summary_status: "ready",
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
        conversation_index: 1,
        conversation_total: 1,
        result_summary_status: "unavailable",
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
        conversation_index: 1,
        conversation_total: 1,
        result_summary_status: "unavailable",
      }],
      [],
    );

    expect(nodes).toEqual([]);
  });
});
