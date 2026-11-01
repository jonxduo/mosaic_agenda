import { defineConfig } from '@playwright/test';

// Three local devices (api-dev-server on 8791..8793, neutral "club" fixture)
// behind one Vite server; each page picks its device with `?api_port=`.
const UI_PORT = 5174;

export default defineConfig({
  testDir: 'tests/e2e',
  timeout: 180_000,
  expect: { timeout: 20_000 },
  fullyParallel: false,
  workers: 1,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: { baseURL: `http://localhost:${UI_PORT}`, locale: 'it-IT', trace: 'retain-on-failure' },
  webServer: [
    {
      command: 'node tests/e2e/devices.mjs 8791 8792 8793',
      port: 8793,
      timeout: 900_000,
      reuseExistingServer: false,
      stdout: 'pipe'
    },
    {
      command: `vite dev --port ${UI_PORT}`,
      port: UI_PORT,
      env: { API_URL: 'http://127.0.0.1:8791', UI_PORT: String(UI_PORT) },
      reuseExistingServer: false
    }
  ]
});
