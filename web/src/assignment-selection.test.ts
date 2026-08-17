import { describe, expect, it } from "vitest";

import type { ActivityDetail, OriginSummary } from "./api-contracts";
import {
  getAssignmentSelection,
  type AssignmentSelection,
} from "./assignment-selection";

const origins: OriginSummary[] = [
  {
    id: 18, display_path: "C:\\shared", kind: "directory",
    resolution_source: "legacy_migrated", setup_state: "confirmed", routing_mode: "dedicated",
    default_project_id: 3, default_project_name: "Legacy", activity_count: 1,
    conversation_count: 1, recommended_mode: "dedicated",
  },
  {
    id: 19, display_path: "C:\\shared", kind: "git",
    resolution_source: "captured", setup_state: "confirmed", routing_mode: "dedicated",
    default_project_id: 3, default_project_name: "Git", activity_count: 1,
    conversation_count: 1, recommended_mode: "dedicated",
  },
  {
    id: 99, display_path: "C:\\shared", kind: "directory",
    resolution_source: "captured", setup_state: "confirmed", routing_mode: "dedicated",
    default_project_id: 9, default_project_name: "Collision", activity_count: 2,
    conversation_count: 1, recommended_mode: "dedicated",
  },
  {
    id: 17, display_path: "C:\\shared", kind: "directory",
    resolution_source: "captured", setup_state: "confirmed", routing_mode: "shared",
    default_project_id: null, default_project_name: null, activity_count: 2,
    conversation_count: 1, recommended_mode: "dedicated",
  },
  {
    id: 42, display_path: "C:\\dedicated", kind: "git",
    resolution_source: "captured", setup_state: "confirmed", routing_mode: "dedicated",
    default_project_id: 4, default_project_name: "Dedicated", activity_count: 1,
    conversation_count: 1, recommended_mode: "dedicated",
  },
];

function detail(
  id: number,
  provider: string,
  sessionId: string,
  origin: ActivityDetail["origin"],
): ActivityDetail {
  return {
    id, provider, activity_kind: "user", prompt: `prompt ${id}`, project: null,
    captured_at: { value: null, provenance: "unknown" },
    first_recorded_at: { value: null, provenance: "unknown" },
    on_canvas: true, submitted_cwd: null, origin,
    technical: {
      session_id: sessionId,
      turn_id: `turn-${id}`,
      agent_id: null,
      agent_type: null,
    },
    result_summary: { status: "unavailable", lines: null, can_regenerate: false },
    prompt_summary: { status: "unavailable", mode: "fallback", text: null },
    selected_turn: {
      id, activity_kind: "user", prompt: `prompt-${id}`, project: null,
      time: { value: null, provenance: "unknown" }, on_canvas: true, selected: true,
      result_summary: { status: "unavailable", lines: null, can_regenerate: false },
      prompt_summary: { status: "unavailable", mode: "fallback", text: null },
    },
    conversation: [], conversation_index: 1, conversation_total: 0, conversation_has_more: false,
  };
}

const sharedOrigin: ActivityDetail["origin"] = {
  id: 17,
  kind: "directory", resolution_source: "captured", display_path: "C:\\shared",
  activity_count: 2,
};
const dedicatedOrigin: ActivityDetail["origin"] = {
  id: 42,
  kind: "git", resolution_source: "captured", display_path: "C:\\dedicated",
  activity_count: 1,
};

type UnionKeys<T> = T extends T ? keyof T : never;
type TechnicalResultKey = Extract<
  UnionKeys<AssignmentSelection>,
  "session" | "sessionId" | "session_id"
>;
type AssertNever<T extends never> = T;
type _SelectionNeverReturnsTechnicalSession = AssertNever<TechnicalResultKey>;

const first = detail(1, "codex", "session-a", sharedOrigin);

describe("getAssignmentSelection", () => {
  it("distinguishes loading and an empty selection", () => {
    expect(getAssignmentSelection(undefined, origins)).toEqual({ state: "loading" });
    expect(getAssignmentSelection([], origins)).toEqual({ state: "empty" });
  });

  it("makes one or many shared activities assignable and defaults future routing off", () => {
    const second = detail(2, "codex", "session-a", sharedOrigin);

    expect(getAssignmentSelection([first], origins)).toEqual({
      state: "assignable", activityIds: [1], futureRoute: { defaultChecked: false },
    });
    const selection = getAssignmentSelection([first, second], origins);
    expect(selection).toEqual({
      state: "assignable", activityIds: [1, 2], futureRoute: { defaultChecked: false },
    });
    expect(JSON.stringify(selection)).not.toContain("session-a");
  });

  it("hides future routing when provider or technical session differs", () => {
    const differentSession = detail(2, "codex", "session-b", sharedOrigin);
    const differentProvider = detail(3, "claude", "session-a", sharedOrigin);

    for (const selection of [[first, differentSession], [first, differentProvider]]) {
      expect(getAssignmentSelection(selection, origins)).toEqual({
        state: "assignable", activityIds: selection.map(({ id }) => id), futureRoute: null,
      });
    }
  });

  it("returns the matching dedicated origin guardrail", () => {
    const dedicated = detail(3, "codex", "session-a", dedicatedOrigin);

    expect(getAssignmentSelection([dedicated], origins)).toEqual({
      state: "dedicated", originId: 42,
    });
  });

  it("blocks a shared and dedicated selection without guessing a route", () => {
    const dedicated = detail(3, "codex", "session-a", dedicatedOrigin);

    expect(getAssignmentSelection([first, dedicated], origins)).toEqual({ state: "blocked" });
  });
});
