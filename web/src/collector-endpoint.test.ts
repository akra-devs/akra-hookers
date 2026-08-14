import { describe, expect, it } from "vitest";

import { validateCollectorEndpoint } from "./collector-endpoint";

describe("validateCollectorEndpoint", () => {
  it.each([
    ["http://localhost:3000", "http://localhost:3000"],
    ["http://127.0.0.1:3103/", "http://127.0.0.1:3103"],
    ["http://127.255.10.2:8080", "http://127.255.10.2:8080"],
    ["http://[::1]:4173", "http://[::1]:4173"],
  ])("classifies the exact loopback space as local: %s", (input, endpoint) => {
    expect(validateCollectorEndpoint(input)).toEqual({
      ok: true,
      value: { endpoint, origin: endpoint, mode: "local" },
    });
  });

  it("accepts only HTTPS for an external collector", () => {
    expect(validateCollectorEndpoint("https://collector.example.com:8443")).toEqual({
      ok: true,
      value: {
        endpoint: "https://collector.example.com:8443",
        origin: "https://collector.example.com:8443",
        mode: "remote",
      },
    });
    expect(validateCollectorEndpoint("http://collector.example.com")).toEqual({
      ok: false,
      error: "외부 수집 주소는 HTTPS를 사용해야 합니다.",
    });
    expect(validateCollectorEndpoint("http://localhost.example.com:3000")).toEqual({
      ok: false,
      error: "외부 수집 주소는 HTTPS를 사용해야 합니다.",
    });
  });

  it.each([
    ["https://user:secret@collector.example.com", "주소에 사용자 이름이나 비밀번호를 포함할 수 없습니다."],
    ["https://collector.example.com/v1", "Collector URL에는 경로, query 또는 hash를 추가할 수 없습니다."],
    ["https://collector.example.com?region=kr", "Collector URL에는 경로, query 또는 hash를 추가할 수 없습니다."],
    ["https://collector.example.com?", "Collector URL에는 경로, query 또는 hash를 추가할 수 없습니다."],
    ["https://collector.example.com#status", "Collector URL에는 경로, query 또는 hash를 추가할 수 없습니다."],
    ["https://collector.example.com#", "Collector URL에는 경로, query 또는 hash를 추가할 수 없습니다."],
    ["https://collector.example.com:0", "포트 0은 사용할 수 없습니다."],
  ])("rejects unsafe endpoint input: %s", (input, error) => {
    expect(validateCollectorEndpoint(input)).toEqual({ ok: false, error });
  });

  it("does not reinterpret deceptive or numeric hosts as loopback", () => {
    for (const input of [
      "http://localhost.",
      "http://127.0.0.1.evil.example",
      "http://2130706433",
      "http://127.0.0.01",
      "https://localhost.",
    ]) {
      expect(validateCollectorEndpoint(input).ok).toBe(false);
    }
  });
});
