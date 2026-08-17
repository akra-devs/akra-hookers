import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import { createServer } from "node:http";
import path from "node:path";

const CONTENT_TYPES = new Map([
  [".css", "text/css; charset=utf-8"],
  [".html", "text/html; charset=utf-8"],
  [".ico", "image/x-icon"],
  [".jpeg", "image/jpeg"],
  [".jpg", "image/jpeg"],
  [".js", "text/javascript; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".png", "image/png"],
  [".svg", "image/svg+xml"],
  [".webp", "image/webp"],
  [".woff", "font/woff"],
  [".woff2", "font/woff2"],
]);

function nonEmptyPath(value) {
  return typeof value === "string" && value.trim() !== "" ? value : null;
}

export function resolveAkraDataDirectory({
  platform,
  environment = {},
  homeDirectory,
  fallbackDataDirectory,
}) {
  const pathApi = platform === "win32" ? path.win32 : path.posix;
  const configured = nonEmptyPath(environment.AKRA_HOOKERS_DATA_DIR);
  if (configured) return pathApi.normalize(configured);

  const home = nonEmptyPath(homeDirectory);
  let baseDirectory;
  switch (platform) {
    case "win32":
      baseDirectory = nonEmptyPath(environment.LOCALAPPDATA)
        ?? (home ? pathApi.join(home, "AppData", "Local") : null);
      break;
    case "darwin":
    case "ios":
      baseDirectory = home ? pathApi.join(home, "Library", "Application Support") : null;
      break;
    case "linux":
      baseDirectory = nonEmptyPath(environment.XDG_DATA_HOME)
        ?? (home ? pathApi.join(home, ".local", "share") : null);
      break;
    default:
      baseDirectory = nonEmptyPath(environment.XDG_DATA_HOME)
        ?? nonEmptyPath(fallbackDataDirectory)
        ?? (home ? pathApi.join(home, ".local", "share") : null);
  }

  if (!baseDirectory) throw new Error(`Akra data directory is unavailable for ${platform}.`);
  return pathApi.join(baseDirectory, "akra-hookers");
}

export function parseRuntimeReady(line) {
  const match = /^ready url=(http:\/\/\S+) token=(akra-[A-Za-z0-9-]+)$/.exec(line.trim());
  if (!match) return null;
  try {
    const url = new URL(match[1]);
    if (url.protocol !== "http:" || url.hostname !== "127.0.0.1") return null;
    return { apiUrl: url.origin, token: match[2] };
  } catch {
    return null;
  }
}

export function isSafeExternalUrl(value) {
  try {
    const protocol = new URL(value).protocol;
    return protocol === "http:" || protocol === "https:";
  } catch {
    return false;
  }
}

export function resolveStaticAsset(root, requestUrl) {
  let pathname;
  try {
    pathname = decodeURIComponent(new URL(requestUrl, "http://127.0.0.1").pathname);
  } catch {
    return null;
  }
  const relative = pathname === "/" ? "index.html" : pathname.replace(/^\/+/, "");
  const resolved = path.resolve(root, relative);
  const normalizedRoot = `${path.resolve(root)}${path.sep}`;
  return resolved.startsWith(normalizedRoot) ? resolved : null;
}

export async function startStaticServer(root) {
  const indexPath = path.join(root, "index.html");
  await stat(indexPath);
  const server = createServer(async (request, response) => {
    if (request.method !== "GET" && request.method !== "HEAD") {
      response.writeHead(405, { Allow: "GET, HEAD" }).end();
      return;
    }
    let assetPath = resolveStaticAsset(root, request.url ?? "/");
    if (!assetPath) {
      response.writeHead(400).end();
      return;
    }
    try {
      if (!(await stat(assetPath)).isFile()) throw new Error("not a file");
    } catch {
      assetPath = indexPath;
    }
    response.setHeader("Content-Type", CONTENT_TYPES.get(path.extname(assetPath)) ?? "application/octet-stream");
    response.setHeader("Cache-Control", assetPath === indexPath ? "no-store" : "public, max-age=31536000, immutable");
    response.setHeader("X-Content-Type-Options", "nosniff");
    if (request.method === "HEAD") {
      response.writeHead(200).end();
      return;
    }
    createReadStream(assetPath).on("error", () => response.destroy()).pipe(response);
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("dashboard listener has no TCP address");
  return {
    origin: `http://127.0.0.1:${address.port}`,
    close: () => new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve())),
  };
}
