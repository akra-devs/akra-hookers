import { copyFile, mkdir, readdir, stat } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const showcaseDirectory = dirname(fileURLToPath(import.meta.url));
const artifactDirectory = resolve(showcaseDirectory, "../../artifacts");
const playwrightDirectory = join(artifactDirectory, "showcase-playwright");

async function findVideos(directory) {
  const videos = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) videos.push(...await findVideos(path));
    if (entry.isFile() && entry.name === "video.webm") {
      videos.push({ path, modified: (await stat(path)).mtimeMs });
    }
  }
  return videos;
}

await mkdir(artifactDirectory, { recursive: true });
const videos = await findVideos(playwrightDirectory);
const source = videos.sort((left, right) => right.modified - left.modified)[0]?.path;
if (!source) throw new Error(`No Playwright video was found under ${playwrightDirectory}`);

const webm = join(artifactDirectory, "Akra-Hookers-Showcase-QHD.webm");
const mp4 = join(artifactDirectory, "Akra-Hookers-Showcase-QHD.mp4");
await copyFile(source, webm);

const localAppData = process.env.LOCALAPPDATA;
const candidates = [
  "ffmpeg",
  localAppData ? join(localAppData, "Microsoft", "WinGet", "Links", "ffmpeg.exe") : null,
].filter(Boolean);
let result;
for (const executable of candidates) {
  result = spawnSync(executable, [
    "-y",
    "-i", webm,
    "-c:v", "libx264",
    "-preset", "slow",
    "-crf", "17",
    "-pix_fmt", "yuv420p",
    "-movflags", "+faststart",
    "-an",
    mp4,
  ], { stdio: "inherit", windowsHide: true });
  if (!result.error && result.status === 0) break;
}
if (!result || result.error || result.status !== 0) {
  throw result?.error ?? new Error(`ffmpeg exited with status ${result?.status ?? "unknown"}`);
}

console.log(`Showcase WebM: ${webm}`);
console.log(`Showcase MP4:  ${mp4}`);
