import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig, type ProxyOptions } from 'vite';

// In the browser the UI talks to `api-dev-server` through /api (see src/lib/bridge.ts).
const api = process.env.API_URL ?? 'http://127.0.0.1:8787';

// `/d/<port>/api/...` reaches another local dev server (ports 8787-8799): one
// browser can act as several devices (`?api_port=<port>`), which the
// multi-device E2E tests use. Loopback only.
const proxy: Record<string, string | ProxyOptions> = { '/api': api };
for (let port = 8787; port <= 8799; port++) {
  proxy[`/d/${port}/api`] = { target: `http://127.0.0.1:${port}`, rewrite: (p) => p.replace(`/d/${port}`, '') };
}

export default defineConfig({
  plugins: [sveltekit()],
  clearScreen: false,
  server: { port: Number(process.env.UI_PORT ?? 5173), strictPort: true, proxy },
  preview: { port: Number(process.env.UI_PORT ?? 4173), strictPort: true, proxy }
});
