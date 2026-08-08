import { describe, expect, it } from "vitest";

import { toCanvasNodes } from "./canvas";

describe("toCanvasNodes", () => {
  it("creates a stable removable node per immutable activity", () => {
    const nodes = toCanvasNodes(
      [
        {
          id: 9,
          provider: "codex",
          session_id: "session",
          turn_id: "turn",
          prompt: "Add a health endpoint",
        },
      ],
      [{ id: 3, activity_event_id: 9, position_x: 64, position_y: 64 }],
    );

    expect(nodes).toMatchObject([
      {
        id: "activity-9",
        position: { x: 64, y: 64 },
        data: { provider: "codex", prompt: "Add a health endpoint" },
      },
    ]);
  });

  it("uses durable canvas positions when supplied by the API", () => {
    const nodes = toCanvasNodes(
      [{ id: 9, provider: "codex", session_id: "s", turn_id: "t", prompt: "keep" }],
      [{ id: 3, activity_event_id: 9, position_x: 120, position_y: 220 }],
    );

    expect(nodes[0]?.position).toEqual({ x: 120, y: 220 });
  });

  it("does not recreate an activity whose removable canvas node is absent", () => {
    const nodes = toCanvasNodes(
      [{ id: 9, provider: "codex", session_id: "s", turn_id: "t", prompt: "removed" }],
      [],
    );

    expect(nodes).toEqual([]);
  });
});
