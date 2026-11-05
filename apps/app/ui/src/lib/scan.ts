import { api, run } from './api';
import { t } from './i18n';
import { toast } from './toast.svelte';

/** Reads a picked file as a `data:` URL. */
export function fileUrl(file: File): Promise<string> {
  return new Promise((ok, ko) => {
    const r = new FileReader();
    r.onload = () => ok(String(r.result));
    r.onerror = () => ko(r.error);
    r.readAsDataURL(file);
  });
}

/** Reads a printed card from a photo; returns the values for the form. */
export async function scanCard(resourceType: string, file: File): Promise<Record<string, unknown>> {
  const res = await run(api.scanResource(resourceType, await fileUrl(file)));
  if (!res.text_read) toast(t('scan_no_text'));
  else if (res.missing.length) toast(t('scan_missing').replace('{list}', res.missing.join(', ')));
  else toast(t('scan_done'));
  return res.values as Record<string, unknown>;
}
