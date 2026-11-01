// In the Tauri webview `window.__TAURI_INTERNALS__` exists and commands go
// through IPC. In a browser (development, Playwright) the same commands are
// sent to `api-dev-server` through /api, so the generated bindings work
// unchanged against a real core.
export function installBrowserBridge(): boolean {
  if (typeof window === 'undefined' || window.__TAURI_INTERNALS__) return false;
  // `?api_port=<port>` (8787-8799) picks another local dev server: several devices in one browser.
  const port = new URLSearchParams(location.search).get('api_port');
  const base = port && /^87(8[7-9]|9\d)$/.test(port) ? `/d/${port}` : '';
  window.__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args?: unknown) => {
      const res = await fetch(`${base}/api/${cmd}`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(args ?? {})
      });
      const body = await res.json().catch(() => null);
      if (!res.ok) throw body ?? { kind: 'network', message: res.statusText };
      return body;
    },
    transformCallback: () => 0,
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } }
  };
  return true;
}
