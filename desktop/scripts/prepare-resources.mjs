import { access, cp, mkdir, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const desktopRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = path.resolve(desktopRoot, "..");
const rendererTarget = path.join(desktopRoot, "renderer");
const binTarget = path.join(desktopRoot, "resources", "bin");
const executable = process.platform === "win32" ? "akra-hookers.exe" : "akra-hookers";
const sidecarSource = path.join(repoRoot, "target", "release", executable);
const webSource = path.join(repoRoot, "web", "dist");

for (const required of [path.join(webSource, "index.html"), sidecarSource]) await access(required);
for (const target of [rendererTarget, binTarget]) {
  if (!target.startsWith(`${desktopRoot}${path.sep}`)) throw new Error(`unsafe generated path: ${target}`);
  await rm(target, { force: true, recursive: true });
}
await cp(webSource, rendererTarget, { recursive: true });
await mkdir(binTarget, { recursive: true });
await cp(sidecarSource, path.join(binTarget, executable));
