import { describe, expect, it } from "vitest";

import { activityIsInPeriod, localDayStartUs } from "./activity-period";

describe("activity periods", () => {
  it("keeps local calendar today distinct from the rolling last 24 hours", () => {
    const now = new Date(2026, 7, 18, 1, 0, 0);
    const lateYesterday = new Date(2026, 7, 17, 23, 30, 0).toISOString();
    const earlyToday = new Date(2026, 7, 18, 0, 30, 0).toISOString();

    expect(activityIsInPeriod(lateYesterday, "today", now)).toBe(false);
    expect(activityIsInPeriod(lateYesterday, "day", now)).toBe(true);
    expect(activityIsInPeriod(earlyToday, "today", now)).toBe(true);
    expect(localDayStartUs(now)).toBe(new Date(2026, 7, 18).getTime() * 1_000);
  });

  it("does not treat unknown activity time as recent", () => {
    expect(activityIsInPeriod(null, "today")).toBe(false);
    expect(activityIsInPeriod(null, "day")).toBe(false);
    expect(activityIsInPeriod(null, "all")).toBe(true);
  });
});
