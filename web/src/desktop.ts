export {};

declare global {
  interface Window {
    akraDesktop?: Readonly<{
      bootstrap: () => Promise<{ apiUrl: string; token: string }>;
      platform: string;
    }>;
  }
}
