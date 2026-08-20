import { access } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const showcaseDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryDirectory = resolve(showcaseDirectory, "../..");
const artifactDirectory = join(repositoryDirectory, "artifacts");
const silentVideo = join(artifactDirectory, "Akra-Hookers-Showcase-QHD.mp4");
const youtubeVideo = join(
  artifactDirectory,
  "Akra-Hookers-Showcase-QHD-YouTube-NCS.mp4",
);

const musicInput = process.env.SHOWCASE_YOUTUBE_MUSIC;
if (!musicInput) {
  throw new Error(
    "Set SHOWCASE_YOUTUBE_MUSIC to the official NCS download before creating the YouTube version.",
  );
}

const music = isAbsolute(musicInput)
  ? musicInput
  : resolve(repositoryDirectory, musicInput);
await Promise.all([access(silentVideo), access(music)]);

const localAppData = process.env.LOCALAPPDATA;
const executableCandidates = (name) => [
  name,
  localAppData
    ? join(localAppData, "Microsoft", "WinGet", "Links", `${name}.exe`)
    : null,
].filter(Boolean);

function runFirstAvailable(name, args, options = {}) {
  let lastResult;
  for (const executable of executableCandidates(name)) {
    const result = spawnSync(executable, args, {
      encoding: "utf8",
      windowsHide: true,
      ...options,
    });
    lastResult = { executable, result };
    if (!result.error && result.status === 0) return lastResult;
  }

  const detail = lastResult?.result.error?.message
    ?? lastResult?.result.stderr
    ?? `No ${name} executable was found`;
  throw new Error(`${name} failed: ${detail}`);
}

const probe = runFirstAvailable("ffprobe", [
  "-v", "error",
  "-show_entries", "format=duration",
  "-of", "default=noprint_wrappers=1:nokey=1",
  silentVideo,
]);
const duration = Number.parseFloat(probe.result.stdout.trim());
if (!Number.isFinite(duration) || duration <= 3) {
  throw new Error(`Invalid showcase duration: ${probe.result.stdout.trim()}`);
}

const musicStart = Number.parseFloat(process.env.SHOWCASE_MUSIC_START ?? "0");
if (!Number.isFinite(musicStart) || musicStart < 0) {
  throw new Error("SHOWCASE_MUSIC_START must be a non-negative number of seconds.");
}

const fadeIn = 0.8;
const fadeOut = 2.5;
const fadeOutStart = duration - fadeOut;
const trimEnd = musicStart + duration;
const preparation = [
  `atrim=start=${musicStart}:end=${trimEnd}`,
  "asetpts=PTS-STARTPTS",
  `afade=t=in:st=0:d=${fadeIn}`,
  `afade=t=out:st=${fadeOutStart}:d=${fadeOut}`,
].join(",");
const loudnessTarget = "I=-18:TP=-1.5:LRA=7";

const analysis = runFirstAvailable("ffmpeg", [
  "-hide_banner",
  "-nostats",
  "-i", music,
  "-af", `${preparation},loudnorm=${loudnessTarget}:print_format=json`,
  "-f", "null",
  "-",
]);
const jsonMatch = analysis.result.stderr.match(/\{[\s\S]*?"target_offset"[\s\S]*?\}/g)?.at(-1);
if (!jsonMatch) {
  throw new Error("ffmpeg did not return loudness measurements for the NCS track.");
}
const measurements = JSON.parse(jsonMatch);
const normalization = [
  `loudnorm=${loudnessTarget}`,
  `measured_I=${measurements.input_i}`,
  `measured_LRA=${measurements.input_lra}`,
  `measured_TP=${measurements.input_tp}`,
  `measured_thresh=${measurements.input_thresh}`,
  `offset=${measurements.target_offset}`,
  "linear=true",
  "print_format=summary",
].join(":");

const mux = runFirstAvailable("ffmpeg", [
  "-y",
  "-hide_banner",
  "-i", silentVideo,
  "-i", music,
  "-filter_complex", `[1:a]${preparation},${normalization},aresample=48000[audio]`,
  "-map", "0:v:0",
  "-map", "[audio]",
  "-c:v", "copy",
  "-c:a", "aac",
  "-b:a", "192k",
  "-ar", "48000",
  "-t", duration.toFixed(3),
  "-movflags", "+faststart",
  "-metadata:s:a:0", "title=On & On (feat. Daniel Levi) [NCS Release]",
  "-metadata:s:a:0", "artist=Cartoon, Jéja, Daniel Levi",
  "-metadata", "comment=YouTube-only NCS edition; see web/showcase/youtube-description.txt",
  youtubeVideo,
], { stdio: "inherit", encoding: undefined });

if (mux.result.status !== 0) {
  throw new Error(`ffmpeg exited with status ${mux.result.status ?? "unknown"}`);
}

console.log(`Silent release-page source: ${silentVideo}`);
console.log(`YouTube NCS edition:        ${youtubeVideo}`);
