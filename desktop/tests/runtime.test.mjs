import assert from "node:assert/strict";
import test from "node:test";
import path from "node:path";
import { isSafeExternalUrl, parseRuntimeReady, resolveStaticAsset } from "../electron/runtime.mjs";

test("parses only bounded loopback runtime readiness", () => {
  assert.deepEqual(parseRuntimeReady("ready url=http://127.0.0.1:42130 token=akra-test-id"), {
    apiUrl: "http://127.0.0.1:42130",
    token: "akra-test-id",
  });
  assert.equal(parseRuntimeReady("ready url=http://example.com:42130 token=akra-test-id"), null);
  assert.equal(parseRuntimeReady("ready url=http://127.0.0.1:42130 token=secret"), null);
});

test("static asset resolver cannot escape renderer root", () => {
  const root = path.resolve("renderer");
  assert.equal(resolveStaticAsset(root, "/"), path.join(root, "index.html"));
  assert.equal(resolveStaticAsset(root, "/assets/app.js"), path.join(root, "assets", "app.js"));
  assert.equal(resolveStaticAsset(root, "/..%2fsecret"), null);
});

test("external navigation is restricted to web URLs", () => {
  assert.equal(isSafeExternalUrl("https://example.com/docs"), true);
  assert.equal(isSafeExternalUrl("http://127.0.0.1:42130"), true);
  assert.equal(isSafeExternalUrl("file:///C:/secret"), false);
  assert.equal(isSafeExternalUrl("javascript:alert(1)"), false);
});
