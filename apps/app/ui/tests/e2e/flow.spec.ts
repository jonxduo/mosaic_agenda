// F5 acceptance in the UI: three devices, from the individual document to
// the distributed aggregate. Uses the neutral "club" fixture (no domain terms).
import { test, expect } from '@playwright/test';
import { DEVICES, call, device, open, lastPeriod, syncUntil } from './helpers';

type Status = { state: string; me: { person_id: string } | null };
type Invitation = { ticket: string };
type Queue = { batches: { period: string; missing: unknown[]; received: unknown[] }[] };

test('three devices: document → batch → aggregate', async ({ browser, request }) => {
  const { a, b, c } = DEVICES;
  const period = lastPeriod();

  // Device A creates the organization through the onboarding.
  const pa = await device(browser, a);
  await pa.getByRole('button', { name: 'Ho letto' }).click();
  await pa.getByLabel("Nome dell'organizzazione").fill('Club');
  await pa.getByLabel('Il tuo nome').fill('Alice Admin');
  await pa.getByRole('button', { name: 'Crea', exact: true }).click();
  await expect(pa.getByRole('navigation')).toContainText('Club');

  // Organization setup (admin actions, through the API to keep the test short).
  const me = (await call<Status>(request, a, 'status')).me!.person_id;
  await call(request, a, 'update_person', { person: me, name: null, attributes: { level: 3 } });
  const team = await call<string>(request, a, 'create_unit', { unitType: 'team', name: 'Team 1' });
  for (const q of ['member', 'senior']) await call(request, a, 'grant_qualification', { person: me, qualification: q, from: '2020-01-01', until: null });
  await call(request, a, 'set_membership', { person: me, unit: team, from: '2020-01-01' });
  await call(request, a, 'grant_role', { person: me, role: 'lead', unit: team });
  await call(request, a, 'grant_role', { person: me, role: 'keeper', unit: null });
  const bob = await call<string>(request, a, 'add_person', { name: 'Bob', attributes: { level: 1 } });
  await call(request, a, 'grant_qualification', { person: bob, qualification: 'member', from: '2020-01-01', until: null });
  await call(request, a, 'set_membership', { person: bob, unit: team, from: '2020-01-01' });
  const carol = await call<string>(request, a, 'add_person', { name: 'Carol', attributes: { level: 1 } });
  await call(request, a, 'grant_qualification', { person: carol, qualification: 'member', from: '2020-01-01', until: null });
  await call(request, a, 'grant_role', { person: carol, role: 'keeper', unit: null });

  // Device B joins with an invitation; A confirms the matching code in the UI.
  const inv = await call<Invitation>(request, a, 'invite_device', { person: bob });
  const pb = await device(browser, b);
  await pb.getByRole('button', { name: 'Ho letto' }).click();
  await pb.getByLabel("Incolla il codice d'invito").fill(inv.ticket);
  await pb.getByRole('button', { name: 'Unisciti' }).click();
  const code = (await pb.getByTestId('pair-code').textContent())!.trim();
  await open(pa, a, '#/sync_status');
  await expect(pa.getByText(code)).toBeVisible({ timeout: 30_000 });
  await pa.getByRole('button', { name: 'Il codice coincide' }).click();
  await expect(pb.getByRole('navigation')).toContainText('Club', { timeout: 60_000 });

  // Device C (Carol) joins too, all through the API.
  const inv2 = await call<Invitation>(request, a, 'invite_device', { person: carol });
  await call(request, c, 'accept_privacy');
  const code2 = await call<string>(request, c, 'join_org', { ticket: inv2.ticket });
  await expect.poll(async () => (await call<{ code: string }[]>(request, a, 'pairings')).some((p) => p.code === code2), { timeout: 30_000 }).toBeTruthy();
  await call(request, a, 'confirm_pairing', { code: code2, accept: true });
  await expect.poll(async () => (await call<Status>(request, c, 'status')).state, { timeout: 60_000 }).toBe('ready');

  // Bob fills and sends his log from the home screen.
  await open(pb, b, '#/home');
  await expect(pb.getByText(/Monthly log/).first()).toBeVisible();
  await pb.getByRole('button', { name: 'Compila' }).first().click();
  await pb.getByRole('radio', { name: 'Sì' }).check();
  await pb.getByRole('spinbutton').first().fill('2');
  await pb.getByRole('button', { name: 'Send' }).click();
  await expect(pb.getByTestId('state')).toBeVisible();

  // Alice sends hers.
  await open(pa, a, `#/form?form=log&period=${period}`);
  await pa.getByRole('radio', { name: 'No' }).check();
  await pa.getByRole('spinbutton').first().fill('1');
  await pa.getByRole('button', { name: 'Send' }).click();
  await expect(pa.getByTestId('state')).toBeVisible();

  // The batch is complete on A after syncing with B.
  await syncUntil(request, [b, a], async () => {
    const q = await call<Queue>(request, a, 'work_queue', { workflow: 'log', unitType: null });
    return q.batches.some((x) => x.period === period && x.missing.length === 0 && x.received.length === 2);
  });

  // A forwards and files the batch from the work queue.
  await open(pa, a, '#/work_queue?workflow=log');
  await pa.getByRole('button', { name: 'Forward' }).click();
  await pa.getByRole('button', { name: 'File' }).click();
  await expect(pa.getByRole('button', { name: 'File' })).toHaveCount(0);

  // A generates and distributes the aggregate.
  await open(pa, a, `#/aggregate?aggregate=log_summary&period=${period}`);
  await pa.getByRole('button', { name: 'Genera' }).click();
  await expect(pa.getByText('Versione 1')).toBeVisible();
  await pa.getByRole('button', { name: 'Distribuisci' }).click();
  await expect(pa.getByText('Distribuito', { exact: true })).toBeVisible();

  // Carol, on device C, sees the distributed aggregate with the count sum 3.
  await syncUntil(request, [a, c], async () => {
    const v = await call<{ distributed: boolean }>(request, c, 'aggregate', { aggregate: 'log_summary', period });
    return v.distributed;
  });
  const pc = await device(browser, c, `#/aggregate?aggregate=log_summary&period=${period}`);
  await expect(pc.getByText('Distribuito', { exact: true })).toBeVisible();
  await expect(pc.getByText('Documenti registrati: 2')).toBeVisible();
  await expect(pc.getByRole('row', { name: /Count\s+3/ })).toHaveCount(2); // total and team

  // Bob, not in the aggregate's audience, sees nothing.
  await open(pb, b, `#/aggregate?aggregate=log_summary&period=${period}`);
  await expect(pb.getByText('Nessun dato visibile')).toBeVisible();
});

// F8: the guided configuration editor changes a role and renames a slot,
// adds the migration by itself, simulates and publishes a new version.
test('guided configuration editor publishes a new version', async ({ browser, request }) => {
  const { a } = DEVICES;
  const page = await device(browser, a);
  await open(page, a, '#/config');
  await expect(page.getByRole('heading', { name: 'Configurazione' })).toBeVisible();

  const lead = page.locator('details.item', { has: page.locator('summary code', { hasText: /^lead$/ }) });
  await lead.locator('summary').click();
  await lead.getByLabel('Titolari massimi').fill('3');
  await lead.getByLabel('Titolari massimi').blur();

  // Slots tab: the host slot becomes the chair.
  await page.getByRole('tab', { name: 'Slot dei programmi' }).click();
  const host = page.locator('details.item', { has: page.locator('summary code', { hasText: /^host$/ }) });
  await host.locator('summary').click();
  await host.getByLabel('Identificativo').fill('chair');
  await host.getByLabel('Identificativo').blur();

  await page.getByRole('button', { name: 'Simula' }).click();
  await expect(page.getByText(/host → .*chair/)).toBeVisible();
  await expect(page.locator('li', { hasText: 'role lead' })).toBeVisible();

  page.on('dialog', (d) => d.accept());
  await page.getByRole('button', { name: /Pubblica versione/ }).click();
  await expect(page.getByText('Versione 2 pubblicata')).toBeVisible();
  const files = await call<{ files: { path: string; text: string }[] }>(request, a, 'config_files');
  expect(files.files.find((f) => f.path === 'package.yaml')!.text).toContain('rename: { from: host, to: chair }');
});

// F9: accessibility (§12.1, contrast AA) of the main views, light and dark.
test('main views have no serious accessibility problems', async ({ browser }) => {
  const { AxeBuilder } = await import('@axe-core/playwright');
  const { a } = DEVICES;
  for (const scheme of ['light', 'dark'] as const) {
    const page = await device(browser, a);
    await page.emulateMedia({ colorScheme: scheme });
    for (const hash of ['#/home', '#/people', '#/settings', '#/config', '#/sync_status']) {
      await open(page, a, hash);
      await page.waitForTimeout(300);
      const result = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
      const serious = result.violations
        .filter((v) => v.impact === 'serious' || v.impact === 'critical')
        .map((v) => `${scheme} ${hash} ${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(' ')).join(' | ')}`);
      expect(serious, serious.join('\n')).toEqual([]);
    }
    await page.close();
  }
});
