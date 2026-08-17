const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("akraDesktop", Object.freeze({
  bootstrap: () => ipcRenderer.invoke("desktop:bootstrap"),
  platform: process.platform,
}));
