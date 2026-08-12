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
  it("shows user and subagent activity while hiding internal work by default", () => {
    expect(isActivityKindVisible("user", DEFAULT_ACTIVITY_VISIBILITY)).toBe(true);
    expect(isActivityKindVisible("subagent", DEFAULT_ACTIVITY_VISIBILITY)).toBe(true);
    expect(isActivityKindVisible("internal", DEFAULT_ACTIVITY_VISIBILITY)).toBe(false);
  });

  it("persists valid independent choices and rejects corrupt preferences", () => {
    const storage = memoryStorage();
    saveActivityVisibility({ subagent: false, internal: true }, storage);
    expect(loadActivityVisibility(storage)).toEqual({ subagent: false, internal: true });

    expect(loadActivityVisibility(memoryStorage("not-json"))).toEqual(
      DEFAULT_ACTIVITY_VISIBILITY,
    );
    expect(loadActivityVisibility(memoryStorage('{"subagent":"yes"}'))).toEqual(
      DEFAULT_ACTIVITY_VISIBILITY,
    );
  });
});
