import { describe, expect, it } from "vitest";

import { formatActivityTime } from "./time";

const captured = {
  value: "2026-08-08T12:00:00Z",
  provenance: "captured",
} as const;

describe("formatActivityTime", () => {
  it("formats an absolute captured time in fixed UTC", () => {
    expect(formatActivityTime(captured, {
      locale: "ko-KR",
      timeZone: "UTC",
    })).toBe("2026. 8. 8. 오후 12:00");
  });

  it("formats the same instant in the browser's Seoul zone", () => {
    expect(formatActivityTime(captured, {
      locale: "ko-KR",
      timeZone: "Asia/Seoul",
    })).toBe("2026. 8. 8. 오후 9:00");
  });

  it("labels migrated timestamps without claiming capture precision", () => {
    expect(formatActivityTime({
      ...captured,
      provenance: "legacy_recorded",
    }, {
      locale: "ko-KR",
      timeZone: "UTC",
    })).toBe("기존 기록 · 2026. 8. 8. 오후 12:00");
  });

  it("uses one truthful label for unknown or absent time", () => {
    expect(formatActivityTime({
      value: "2026-08-08T12:00:00Z",
      provenance: "unknown",
    })).toBe("시간 정보 없음");
    expect(formatActivityTime({
      value: null,
      provenance: "captured",
    })).toBe("시간 정보 없음");
  });
});
