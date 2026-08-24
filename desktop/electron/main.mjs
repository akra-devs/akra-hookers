import { app, BrowserWindow, dialog, ipcMain, Menu, shell } from "electron";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { access, chmod, copyFile, mkdir, rename, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  isSafeExternalUrl,
  parseRuntimeReady,
  resolveAkraDataDirectory,
  startStaticServer,
} from "./runtime.mjs";

const DESKTOP_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SIDECAR_NAME = process.platform === "win32" ? "akra-hookers.exe" : "akra-hookers";
const STARTUP_TIMEOUT_MS = 30_000;
const APP_DATA_ROOT = resolveAkraDataDirectory({
  platform: process.platform,
  environment: process.env,
  homeDirectory: app.getPath("home"),
  fallbackDataDirectory: app.getPath("appData"),
});
app.setPath("userData", path.join(APP_DATA_ROOT, "electron"));
let mainWindow = null;
let runtimeProcess = null;
let runtimeCredentials = null;
let rendererServer = null;
let quitting = false;

function bundledSidecarPath() {
  if (process.env.AKRA_DESKTOP_SIDECAR_PATH) return path.resolve(process.env.AKRA_DESKTOP_SIDECAR_PATH);
  return app.isPackaged
    ? path.join(process.resourcesPath, "bin", SIDECAR_NAME)
    : path.join(DESKTOP_ROOT, "resources", "bin", SIDECAR_NAME);
}

async function digest(file) {
  return await new Promise((resolve, reject) => {
    const hash = createHash("sha256");
    createReadStream(file).on("error", reject).on("data", (chunk) => hash.update(chunk)).on("end", () => resolve(hash.digest("hex")));
  });
}

async function installStableSidecar() {
  const source = bundledSidecarPath();
  await access(source);
  const binDir = path.join(APP_DATA_ROOT, "bin");
  const destination = path.join(binDir, SIDECAR_NAME);
  await mkdir(binDir, { recursive: true });
  try {
    if ((await digest(source)) === (await digest(destination))) return destination;
  } catch {
    // The first launch has no stable copy yet.
  }
  const temporary = `${destination}.${process.pid}.tmp`;
  const backup = `${destination}.bak`;
  await copyFile(source, temporary);
  if (process.platform !== "win32") await chmod(temporary, 0o700);
  await rm(backup, { force: true });
  let hadExisting = false;
  try {
    await rename(destination, backup);
    hadExisting = true;
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
  try {
    await rename(temporary, destination);
    await rm(backup, { force: true });
  } catch (error) {
    await rm(temporary, { force: true });
    if (hadExisting) await rename(backup, destination);
    throw error;
  }
  return destination;
}

async function startRuntime() {
  const executable = await installStableSidecar();
  const dataDir = APP_DATA_ROOT;
  await mkdir(dataDir, { recursive: true });
  const child = spawn(executable, ["serve", "--bind", "127.0.0.1", "--port", "0", "--data-dir", dataDir], {
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
  });
  runtimeProcess = child;
  let stdout = "";
  let stderr = "";
  return await new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      child.kill();
      reject(new Error("Akra runtime did not become ready in time."));
    }, STARTUP_TIMEOUT_MS);
    const fail = (error) => {
      clearTimeout(timer);
      reject(error);
    };
    child.once("error", fail);
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk) => { stderr = `${stderr}${chunk}`.slice(-16_384); });
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
      let boundary;
      while ((boundary = stdout.indexOf("\n")) >= 0) {
        const line = stdout.slice(0, boundary).replace(/\r$/, "");
        stdout = stdout.slice(boundary + 1);
        const ready = parseRuntimeReady(line);
        if (ready) {
          clearTimeout(timer);
          resolve(ready);
          return;
        }
      }
    });
    child.once("exit", (code) => {
      runtimeProcess = null;
      if (!runtimeCredentials) fail(new Error(`Akra runtime exited during startup (${code ?? "unknown"}). ${stderr.trim()}`));
      else if (!quitting) app.quit();
    });
  });
}

function assertOwnedWebContents(event) {
  if (!mainWindow || event.sender.id !== mainWindow.webContents.id) throw new Error("Untrusted renderer request.");
}

async function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1440,
    height: 920,
    minWidth: 900,
    minHeight: 640,
    show: false,
    autoHideMenuBar: true,
    backgroundColor: "#081012",
    title: "Akra Hookers",
    webPreferences: {
      preload: path.join(DESKTOP_ROOT, "electron", "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      webSecurity: true,
    },
  });
  mainWindow.setMenu(null);
  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    if (isSafeExternalUrl(url)) void shell.openExternal(url);
    return { action: "deny" };
  });
  mainWindow.webContents.on("will-navigate", (event, url) => {
    if (url !== rendererServer.origin && !url.startsWith(`${rendererServer.origin}/`)) event.preventDefault();
  });
  mainWindow.once("ready-to-show", () => {
    if (!process.argv.includes("--smoke-test")) mainWindow.show();
  });
  mainWindow.on("closed", () => { mainWindow = null; });
  await mainWindow.loadURL(rendererServer.origin);
  if (process.argv.includes("--smoke-test")) {
    if (mainWindow.isMenuBarVisible()) throw new Error("Desktop menu bar must remain hidden.");
    const heading = await mainWindow.webContents.executeJavaScript("document.querySelector('h1')?.textContent ?? ''");
    if (heading.trim() !== "Prompt canvas") throw new Error(`Unexpected dashboard heading: ${heading}`);
    console.log("desktop smoke test passed");
    app.quit();
  }
}

function stopRuntime() {
  quitting = true;
  if (runtimeProcess && !runtimeProcess.killed) runtimeProcess.kill();
  runtimeProcess = null;
  runtimeCredentials = null;
  if (rendererServer) void rendererServer.close().catch(() => {});
  rendererServer = null;
}

if (!app.requestSingleInstanceLock()) app.quit();
else {
  app.on("second-instance", () => {
    if (mainWindow) {
      if (mainWindow.isMinimized()) mainWindow.restore();
      mainWindow.focus();
    }
  });
  ipcMain.handle("desktop:bootstrap", (event) => {
    assertOwnedWebContents(event);
    if (!runtimeCredentials) throw new Error("Akra runtime is not ready.");
    return { ...runtimeCredentials };
  });
  app.whenReady().then(async () => {
    try {
      Menu.setApplicationMenu(null);
      runtimeCredentials = await startRuntime();
      rendererServer = await startStaticServer(path.join(DESKTOP_ROOT, "renderer"));
      await createWindow();
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      dialog.showErrorBox("Akra Hookers could not start", detail);
      app.exit(1);
    }
  });
  app.on("before-quit", stopRuntime);
  app.on("window-all-closed", () => app.quit());
}
