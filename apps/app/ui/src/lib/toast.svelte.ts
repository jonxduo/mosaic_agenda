import { Failure } from './api';
import { t, type Key } from './i18n';
export const toasts = $state<{ id: number; text: string; error: boolean }[]>([]);
let n = 0;
export function toast(text: string, error = false) {
  const id = ++n;
  toasts.push({ id, text, error });
  setTimeout(() => {
    const i = toasts.findIndex((x) => x.id === id);
    if (i >= 0) toasts.splice(i, 1);
  }, error ? 6000 : 3000);
}
const known = ['denied', 'not_found', 'invalid', 'locked', 'network', 'internal', 'wiped'];

/** Shows a localized error; the technical detail only goes to the console. */
export function fail(e: unknown) {
  console.warn(e);
  if (e instanceof Failure) {
    const k = known.includes(e.kind) ? e.kind : 'internal';
    toast(t(`err_${k}` as Key), true);
  } else {
    toast(t('err_internal'), true);
  }
}
