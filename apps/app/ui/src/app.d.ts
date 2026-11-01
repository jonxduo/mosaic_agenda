declare global {
  namespace App {}
  interface Window {
    __TAURI_INTERNALS__?: { invoke: (cmd: string, args?: unknown, options?: unknown) => Promise<unknown> } & Record<string, unknown>;
  }
}
export {};
