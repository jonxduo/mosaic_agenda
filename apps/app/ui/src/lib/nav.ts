// Menu layout, from the kind of view only (generic for every package):
// sections, icons and the items kept in the phone's bottom bar.
import type { MenuItem } from './bindings';

export type Section = 'me' | 'work' | 'admin' | 'system';

const SECTION: Record<string, Section> = {
  home: 'me',
  my_documents: 'me',
  my_assignments: 'me',
  my_resources: 'me',
  board: 'me',
  work_queue: 'work',
  batch: 'work',
  aggregate: 'work',
  program_list: 'work',
  resource_list: 'work',
  people: 'admin',
  units: 'admin',
  roles: 'admin',
  admin_alerts: 'admin',
  config: 'admin',
  sync_status: 'system',
  settings: 'system'
};

export const sectionOf = (m: MenuItem): Section => SECTION[m.view] ?? 'me';

/** Menu items grouped by section, in the package's order within each. */
export function grouped(menu: MenuItem[]): { section: Section; items: MenuItem[] }[] {
  const order: Section[] = ['me', 'work', 'admin', 'system'];
  return order
    .map((section) => ({ section, items: menu.filter((m) => sectionOf(m) === section) }))
    .filter((g) => g.items.length);
}

/** Up to `n` items for the phone's bottom bar: the most used views first. */
export function primary(menu: MenuItem[], n: number): MenuItem[] {
  const rank = ['home', 'my_documents', 'my_assignments', 'work_queue', 'program_list', 'board', 'my_resources'];
  return [...menu]
    .filter((m) => rank.includes(m.view))
    .sort((a, b) => rank.indexOf(a.view) - rank.indexOf(b.view))
    .filter((m, i, all) => all.findIndex((x) => x.view === m.view) === i)
    .slice(0, n);
}

/** 24×24 stroke icons (one path per view kind). */
export const ICONS: Record<string, string> = {
  home: 'M3 10.5 12 3l9 7.5V20a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z',
  my_documents: 'M14 3H6a1 1 0 0 0-1 1v16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V8z M14 3v5h5 M9 13h6 M9 17h6',
  my_assignments: 'M9 11l3 3 8-8 M20 12v7a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h9',
  my_resources: 'M9 4 3 6v14l6-2 6 2 6-2V4l-6 2z M9 4v14 M15 6v14',
  resource_list: 'M9 4 3 6v14l6-2 6 2 6-2V4l-6 2z M9 4v14 M15 6v14',
  board: 'M9 3h6v3H9z M7 5H5a1 1 0 0 0-1 1v14a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1V6a1 1 0 0 0-1-1h-2 M8 12h8 M8 16h5',
  work_queue: 'M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2 M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8 M22 21v-2a4 4 0 0 0-3-3.87 M16 3.13a4 4 0 0 1 0 7.75',
  batch: 'M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2 M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8',
  aggregate: 'M4 20V10 M10 20V4 M16 20v-7 M22 20H2',
  program_list: 'M8 2v4 M16 2v4 M3 9h18 M5 4h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z',
  people: 'M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2 M12 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8',
  units: 'M3 3h7v7H3z M14 3h7v7h-7z M3 14h7v7H3z M14 14h7v7h-7z',
  roles: 'M12 15a6 6 0 1 0 0-12 6 6 0 0 0 0 12 M8.5 13.5 7 22l5-3 5 3-1.5-8.5',
  admin_alerts: 'M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9 M10.3 21a1.94 1.94 0 0 0 3.4 0',
  config: 'M14.7 6.3a4 4 0 0 0-5.4 5.4L3 18v3h3l6.3-6.3a4 4 0 0 0 5.4-5.4l-2.5 2.5-2.4-.6-.6-2.4z',
  sync_status: 'M21 12a9 9 0 0 1-15.5 6.2L3 16 M3 12a9 9 0 0 1 15.5-6.2L21 8 M21 3v5h-5 M3 21v-5h5',
  settings: 'M4 21v-7 M4 10V3 M12 21v-9 M12 8V3 M20 21v-5 M20 12V3 M1 14h6 M9 8h6 M17 16h6',
  admin: 'M12 3 4 6v6c0 5 3.4 8.3 8 9 4.6-.7 8-4 8-9V6z M9 12l2 2 4-4',
  more: 'M5 12h.01 M12 12h.01 M19 12h.01'
};
