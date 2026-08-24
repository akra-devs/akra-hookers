import { describe, expect, it } from "vitest";

import {
  DEFAULT_ACTIVITY_VISIBILITY,
  isActivityKindVisible,
  loadActivityVisibility,
  saveActivityVisibility,
} from "./activity-visibility";

function memoryStorage(initial: string | null = null) {
  let value = initial;
  return {
    getItem: () => value,
    setItem: (_key: string, next: string) => { value = next; },
    value: () => value,
  };
}

describe("activity visibility", () => {
  it("shows user activity while always hiding subagents and hiding internal work by default", () => {
    expect(isActivityKindVisible("user", DEFAULT_ACTIVITY_VISIBILITY)).toBe(true);
    expect(isActivityKindVisible("subagent", DEFAULT_ACTIVITY_VISIBILITY)).toBe(false);
    expect(isActivityKindVisible("internal", DEFAULT_ACTIVITY_VISIBILITY)).toBe(false);
  });

  it("persists the internal choice and drops the retired subagent preference", () => {
    const storage = memoryStorage();
    saveActivityVisibility({ internal: true }, storage);
    expect(loadActivityVisibility(storage)).toEqual({ internal: true });
    expect(loadActivityVisibility(memoryStorage('{"subagent":true,"internal":true}')))
      .toEqual({ internal: true });

    expect(loadActivityVisibility(memoryStorage("not-json"))).toEqual(
      DEFAULT_ACTIVITY_VISIBILITY,
    );
    expect(loadActivityVisibility(memoryStorage('{"internal":"yes"}'))).toEqual(
      DEFAULT_ACTIVITY_VISIBILITY,
    );
  });
});
