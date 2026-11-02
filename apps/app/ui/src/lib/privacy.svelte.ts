// Rights of the persons (§14): export of a person's data as a JSON file or
// a printable page (the system print dialog saves it as PDF), and erasure.
import { tick } from 'svelte';
import { save } from '@tauri-apps/plugin-dialog';
import type { PersonExport_Serialize as PersonExport } from './bindings';
import { api, run } from './api';
import { inBrowser, download } from './backup';

/** The export being printed, rendered by App in the print area. */
export const printing = $state<{ data: PersonExport | null }>({ data: null });

const fileName = (name: string) => `dati-${name.replace(/[^\p{L}\p{N}]+/gu, '-').toLowerCase()}-${new Date().toISOString().slice(0, 10)}.json`;

/** Returns false when the save dialog was cancelled. */
export async function exportJson(person: string, name: string): Promise<boolean> {
  if (!inBrowser()) {
    const path = await save({ defaultPath: fileName(name), filters: [{ name: 'JSON', extensions: ['json'] }] });
    if (!path) return false;
    await run(api.exportPersonTo(person, path));
    return true;
  }
  const data = await run(api.exportPerson(person));
  download(new TextEncoder().encode(JSON.stringify(data, null, 2)), fileName(name), 'application/json');
  return true;
}

export async function printExport(person: string): Promise<void> {
  printing.data = await run(api.exportPerson(person));
  await tick();
  document.documentElement.classList.add('printing');
  try {
    window.print();
  } finally {
    document.documentElement.classList.remove('printing');
    printing.data = null;
  }
}
