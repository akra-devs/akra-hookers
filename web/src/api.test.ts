import { describe, expect, it, vi } from "vitest";

import { createApiClient } from "./api";

describe("createApiClient", () => {
  it("uses the capability token for activity queries", async () => {
    const fetcher = vi.fn().mockResolvedValue(
      new Response(JSON.stringify([{ id: 1, provider: "codex" }]), { status: 200 }),
    );
    const client = createApiClient("http://127.0.0.1:4319", "capability", fetcher);

    await expect(client.activities()).resolves.toEqual([{ id: 1, provider: "codex" }]);
    expect(fetcher).toHaveBeenCalledWith(
      "http://127.0.0.1:4319/v1/activities",
      expect.objectContaining({ headers: { Authorization: "Bearer capability" } }),
    );
  });

  it("surfaces unsuccessful API responses", async () => {
    const client = createApiClient(
      "http://127.0.0.1:4319",
      "capability",
      vi.fn().mockResolvedValue(new Response("denied", { status: 401 })),
    );

    await expect(client.projects()).rejects.toThrow("API request failed: 401");
  });

  it("loads persisted provider state before rendering controls", async () => {
    const fetcher = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ provider: "codex", enabled: false }), { status: 200 }),
    );
    const client = createApiClient("http://127.0.0.1:4319", "capability", fetcher);

    await expect(client.provider("codex")).resolves.toEqual({ provider: "codex", enabled: false });
    expect(fetcher).toHaveBeenCalledWith(
      "http://127.0.0.1:4319/v1/providers/codex",
      expect.objectContaining({ headers: { Authorization: "Bearer capability" } }),
    );
  });

  it("uses the canvas deletion endpoint for a durable clear", async () => {
    const fetcher = vi.fn().mockResolvedValue(new Response(null, { status: 204 }));
    const client = createApiClient("http://127.0.0.1:4319", "capability", fetcher);

    await expect(client.clearCanvas()).resolves.toBeUndefined();
    expect(fetcher).toHaveBeenCalledWith(
      "http://127.0.0.1:4319/v1/canvas",
      expect.objectContaining({ method: "DELETE" }),
    );
  });
});
