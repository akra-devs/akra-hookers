/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_AKRA_URL?: string;
  readonly VITE_AKRA_TOKEN?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
