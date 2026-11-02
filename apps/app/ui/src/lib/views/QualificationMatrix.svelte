<script lang="ts">
  // Persons × qualifications: one row per person, one column per
  // qualification listed by the package; a tick grants it. Changes stay
  // here until "Salva".
  import type { QualificationMatrix, QualificationChange } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail, toast } from '../toast.svelte';

  let { qualifications }: { qualifications?: string } = $props();
  const cols = () => qualifications ?? '';
  let m = $state<QualificationMatrix | null>(null);
  let pending = $state<Record<string, QualificationChange>>({});
  let busy = $state(false);
  let search = $state('');
  let unitFilter = $state('');

  run(api.qualificationMatrix(cols())).then((x) => (m = x)).catch(fail);

  const collator = new Intl.Collator('it', { sensitivity: 'base' });
  const fold = (x: string) => x.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();
  const rows = $derived(
    (m?.rows ?? [])
      .filter((r) => !unitFilter || r.unit === unitFilter)
      .filter((r) => !search.trim() || fold(r.name).includes(fold(search.trim())))
  );
  const units = $derived([...new Set((m?.rows ?? []).map((r) => r.unit).filter((u): u is string => !!u))].sort(collator.compare));
  const changed = $derived(Object.keys(pending).length);
  const key = (person: string, q: string) => `${person}|${q}`;
  const isHeld = (person: string, q: string, held: boolean) => pending[key(person, q)]?.held ?? held;
  const count = (ci: number) => (m?.rows ?? []).filter((r) => isHeld(r.person_id, m!.columns[ci].id, r.cells[ci].held)).length;

  function toggle(person: string, q: string, held: boolean, value: boolean) {
    const k = key(person, q);
    if (value === held) delete pending[k];
    else pending[k] = { person, qualification: q, held: value };
  }

  async function save() {
    busy = true;
    try {
      m = await run(api.setQualifications(cols(), Object.values(pending)));
      pending = {};
      toast(t('saved'));
    } catch (e) {
      fail(e);
      // Part of the changes may have been saved: show what is true now.
      m = await run(api.qualificationMatrix(cols())).catch(() => m);
      pending = {};
    } finally {
      busy = false;
    }
  }
</script>

{#if m}
  <div class="stack page">
    <header class="stack tight">
      <h1>{t('matrix_title')} <span class="count">{rows.length}</span></h1>
      <p class="muted">{t('matrix_hint')}</p>
    </header>

    <div class="row toolbar">
      <input class="grow" type="search" placeholder={t('people_search')} aria-label={t('people_search')} bind:value={search} />
      {#if units.length}
        <select aria-label={t('unit')} bind:value={unitFilter}>
          <option value="">{t('people_all_units')}</option>
          {#each units as u (u)}<option value={u}>{u}</option>{/each}
        </select>
      {/if}
    </div>

    <div class="card scroller">
      <table>
        <thead>
          <tr>
            <th class="who" scope="col">{t('name')}</th>
            {#each m.columns as c, ci (c.id)}
              <th scope="col" class="col" title={c.label}><span class="vertical">{c.label}</span><span class="n">{count(ci)}</span></th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each rows as r (r.person_id)}
            <tr>
              <th class="who" scope="row">
                <span class="name">{r.name}</span>
                {#if r.unit}<span class="muted unit">{r.unit}</span>{/if}
              </th>
              {#each m.columns as c, ci (c.id)}
                {@const cell = r.cells[ci]}
                {@const value = isHeld(r.person_id, c.id, cell.held)}
                <td class:changed={key(r.person_id, c.id) in pending}>
                  <input
                    type="checkbox"
                    checked={value}
                    disabled={!m.can_manage || (!cell.allowed && !cell.held)}
                    title={!cell.allowed && !cell.held ? t('matrix_not_allowed') : ''}
                    aria-label={`${r.name} · ${c.label}`}
                    onchange={(e) => toggle(r.person_id, c.id, cell.held, e.currentTarget.checked)}
                  />
                </td>
              {/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    {#if m.can_manage}
      <div class="row actions">
        <button class="btn" disabled={busy || !changed} onclick={() => (pending = {})}>{t('config_discard')}</button>
        <button class="btn primary" disabled={busy || !changed} onclick={save}>{t('matrix_save')}</button>
        {#if changed}<span class="muted" role="status">{t('unsaved_changes').replace('{n}', String(changed))}</span>{/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .page { gap: 14px; }
  .tight { gap: 4px; }
  .count { font-size: 0.6em; font-weight: 600; color: var(--muted); vertical-align: middle; }
  .toolbar { gap: 10px; }
  .toolbar select { max-width: 220px; }
  .scroller { padding: 0; overflow: auto; max-height: calc(100vh - 260px); }
  table { border-collapse: separate; border-spacing: 0; width: max-content; min-width: 100%; }
  th, td { border-bottom: 1px solid var(--line); }
  thead th { position: sticky; top: 0; background: var(--bg); z-index: 2; vertical-align: bottom; }
  .who { position: sticky; left: 0; background: var(--bg); z-index: 1; text-align: left; padding: 8px 14px; min-width: 180px; max-width: 240px; border-right: 1px solid var(--line); }
  thead .who { z-index: 3; font-size: 14px; color: var(--muted); font-weight: 600; }
  tbody .who { font-weight: 500; }
  .name { display: block; }
  .unit { display: block; font-size: 12px; font-weight: 400; }
  .col { padding: 10px 4px 6px; width: 44px; font-size: 13px; font-weight: 600; }
  .vertical { display: block; writing-mode: vertical-rl; transform: rotate(180deg); margin: 0 auto; max-height: 240px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .n { display: block; margin-top: 6px; font-size: 11px; color: var(--muted); font-weight: 500; text-align: center; }
  td { text-align: center; padding: 6px 4px; }
  td.changed { background: var(--primary-soft); }
  tbody tr:hover td, tbody tr:hover .who { background: var(--surface); }
  tbody tr:hover td.changed { background: var(--primary-soft); }
  td input { width: 20px; height: 20px; min-height: 20px; margin: 0; accent-color: var(--primary); cursor: pointer; }
  td input:disabled { cursor: not-allowed; opacity: 0.35; }
  @media (max-width: 640px) {
    .who { min-width: 130px; }
    .scroller { max-height: calc(100vh - 340px); }
  }
</style>
