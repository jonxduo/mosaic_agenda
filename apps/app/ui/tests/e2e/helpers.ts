import { expect, type APIRequestContext, type Browser, type Page } from '@playwright/test';

export const DEVICES = { a: 8791, b: 8792, c: 8793 } as const;

/** Calls a command on one device directly (setup only; the flow goes through the UI). */
export async function call<T = unknown>(request: APIRequestContext, port: number, cmd: string, args: object = {}): Promise<T> {
  const res = await request.post(`/d/${port}/api/${cmd}`, { data: args });
  const body = await res.json();
  expect(res.ok(), `${cmd}: ${JSON.stringify(body)}`).toBeTruthy();
  return body as T;
}

/** A page acting as the device behind `port`. */
export async function device(browser: Browser, port: number, hash = '#/home'): Promise<Page> {
  const page = await (await browser.newContext({ locale: 'it-IT' })).newPage();
  await page.goto(`/?api_port=${port}${hash}`);
  return page;
}

/** Goes to a view of the app keeping the device selection. */
export async function open(page: Page, port: number, hash: string) {
  await page.goto(`/?api_port=${port}${hash}`);
}

/** The month before today as `YYYY-MM` (the period being reported). */
export function lastPeriod(now = new Date()): string {
  const d = new Date(now.getFullYear(), now.getMonth() - 1, 1);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`;
}

/** Retries `sync_now` on the given devices until `check` passes. */
export async function syncUntil(request: APIRequestContext, ports: number[], check: () => Promise<boolean>) {
  for (let i = 0; i < 60; i++) {
    for (const p of ports) await request.post(`/d/${p}/api/sync_now`, { data: {} });
    if (await check()) return;
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error('condition not reached after syncing');
}
