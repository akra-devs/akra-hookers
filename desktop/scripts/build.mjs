import { packager } from "@electron/packager";
import path from "node:path";
import { fileURLToPath } from "node:url";

const desktopRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const version = process.env.AKRA_DESKTOP_VERSION ?? "0.1.0";
const output = await packager({
  dir: desktopRoot,
  out: path.join(desktopRoot, "dist"),
  overwrite: true,
  platform: process.platform,
  arch: process.arch,
  name: "Akra Hookers",
  executableName: "Akra Hookers",
  appVersion: version,
  asar: true,
  prune: true,
  extraResource: [path.join(desktopRoot, "resources", "bin")],
  ignore: [
    /^\/dist(?:\/|$)/,
    /^\/resources(?:\/|$)/,
    /^\/scripts(?:\/|$)/,
    /^\/tests(?:\/|$)/,
  ],
});
console.log(`desktop package: ${output.join(", ")}`);
