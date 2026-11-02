// Phone-only features: biometric unlock and QR scanning (Tauri plugins that
// exist on Android and iOS only). Elsewhere the buttons are not shown.
import { authenticate } from '@tauri-apps/plugin-biometric';
import { scan, Format } from '@tauri-apps/plugin-barcode-scanner';
import { invoke } from '@tauri-apps/api/core';

export const isMobile = typeof navigator !== 'undefined' && /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);

/** Asks the system for fingerprint or face; throws if refused. */
export async function biometricCheck(reason: string): Promise<void> {
  await authenticate(reason, { allowDeviceCredential: false });
}

/** Opens the camera and returns the text of the first QR code. */
export async function scanQr(): Promise<string> {
  const r = await scan({ formats: [Format.QRCode], windowed: false });
  return r.content;
}

/**
 * Android draws the page under the status and navigation bars: their sizes
 * become CSS variables used as margins (iOS uses env(safe-area-inset-*)).
 */
export async function applyInsets(): Promise<void> {
  if (!isMobile) return;
  try {
    const r = await invoke<{ top?: number; bottom?: number }>('plugin:secret-store|insets');
    const root = document.documentElement.style;
    if (r.top) root.setProperty('--inset-top', `${r.top}px`);
    if (r.bottom) root.setProperty('--inset-bottom', `${r.bottom}px`);
  } catch {
    // Not available (browser, desktop): keep the CSS defaults.
  }
}
