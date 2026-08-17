import assert from "node:assert/strict";
import test from "node:test";
import path from "node:path";
import {
  isSafeExternalUrl,
  parseRuntimeReady,
  resolveAkraDataDirectory,
  resolveStaticAsset,
} from "../electron/runtime.mjs";

test("Windows portable and development builds share LocalAppData", () => {
  const options = {
    platform: "win32",
    environment: {
      LOCALAPPDATA: "C:\\Users\\alex\\AppData\\Local",
      XDG_DATA_HOME: "C:\\wrong-xdg",
    },
    homeDirectory: "C:\\Users\\alex",
    fallbackDataDirectory: "C:\\Users\\alex\\AppData\\Roaming",
  };
  assert.equal(resolveAkraDataDirectory(options), "C:\\Users\\alex\\AppData\\Local\\akra-hookers");
});

test("Ubuntu uses XDG data home with the freedesktop fallback", () => {
  assert.equal(resolveAkraDataDirectory({
    platform: "linux",
    environment: {
      LOCALAPPDATA: "/mnt/c/Users/alex/AppData/Local",
      XDG_DATA_HOME: "/home/alex/.data",
    },
    homeDirectory: "/home/alex",
  }), "/home/alex/.data/akra-hookers");
  assert.equal(resolveAkraDataDirectory({
    platform: "linux",
    environment: { LOCALAPPDATA: "/mnt/c/Users/alex/AppData/Local" },
    homeDirectory: "/home/alex",
  }), "/home/alex/.local/share/akra-hookers");
});

test("Apple hosts use their sandbox Application Support directory", () => {
  for (const platform of ["darwin", "ios"]) {
    assert.equal(resolveAkraDataDirectory({
      platform,
      environment: {},
      homeDirectory: "/Users/alex",
    }), "/Users/alex/Library/Application Support/akra-hookers");
  }
});

test("an explicit Akra data directory overrides every OS default", () => {
  assert.equal(resolveAkraDataDirectory({
    platform: "linux",
    environment: {
      AKRA_HOOKERS_DATA_DIR: "/srv/akra-state",
      XDG_DATA_HOME: "/home/alex/.data",
    },
    homeDirectory: "/home/alex",
  }), "/srv/akra-state");
});

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
