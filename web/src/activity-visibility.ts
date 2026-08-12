import type { ActivityKind } from "./api-contracts";

const STORAGE_KEY = "akra.canvas.activity-visibility.v1";

export type ActivityVisibility = {
  subagent: boolean;
  internal: boolean;
};

export const DEFAULT_ACTIVITY_VISIBILITY: ActivityVisibility = {
  subagent: true,
  internal: false,
};

type VisibilityStorage = Pick<Storage, "getItem" | "setItem">;

export function loadActivityVisibility(
  storage: VisibilityStorage | null = browserStorage(),
): ActivityVisibility {
  if (!storage) return DEFAULT_ACTIVITY_VISIBILITY;
  try {
    const stored = JSON.parse(storage.getItem(STORAGE_KEY) ?? "null") as unknown;
    if (
      typeof stored === "object"
      && stored !== null
      && "subagent" in stored
      && "internal" in stored
      && typeof stored.subagent === "boolean"
      && typeof stored.internal === "boolean"
    ) {
      return { subagent: stored.subagent, internal: stored.internal };
    }
  } catch {
    // Corrupt or unavailable local preferences fall back to the safe defaults.
  }
  return DEFAULT_ACTIVITY_VISIBILITY;
}

export function saveActivityVisibility(
  visibility: ActivityVisibility,
  storage: VisibilityStorage | null = browserStorage(),
) {
  if (!storage) return;
  try {
    storage.setItem(STORAGE_KEY, JSON.stringify(visibility));
  } catch {
    // Canvas behavior must not depend on localStorage being writable.
  }
}

export function isActivityKindVisible(
  kind: ActivityKind,
  visibility: ActivityVisibility,
) {
  if (kind === "subagent") return visibility.subagent;
  if (kind === "internal") return visibility.internal;
  return true;
}

function browserStorage(): VisibilityStorage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
