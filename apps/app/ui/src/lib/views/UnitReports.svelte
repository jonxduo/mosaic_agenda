<script lang="ts">
  // The units one oversees (e.g. a group): its members, read only, and its
  // documents month by month, the period being collected on top.
  import type { UnitReports_Serialize as Data, PersonView_Serialize as Person } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { go } from '../router.svelte';
  import MeasureTable from '../components/MeasureTable.svelte';
  import { fail, toast } from '../toast.svelte';

  let { workflow, unitType, onchange }: { workflow: string; unitType: string; onchange: () => void } = $props();
  let data = $state<Data | null>(null);
  let unitId = $state('');
  // The tab stays as left (per device).
  let tab = $state<'members' | 'reports'>('reports');
  try {
    const saved = localStorage.getItem('unit-reports-tab');
    if (saved === 'members' || saved === 'reports') tab = saved;
  } catch {
    /* no storage: default tab */
  }
  $effect(() => {
    try {
      localStorage.setItem('unit-reports-tab', tab);
    } catch {
      /* no storage */
    }
  });
  let open = $state<string | null>(null);

  async function load() {
    try {
      data = await run(api.unitReports(workflow, unitType));
      if (!data.units.some((u) => u.unit_id === unitId)) unitId = data.units[0]?.unit_id ?? '';
    } catch (e) {
      fail(e);
    }
  }
  load();

  const unit = $derived(data?.units.find((u) => u.unit_id === unitId) ?? null);
  // The month shown: 0 is the newest (the one being collected); the arrows
  // move through the months, the table follows.
  let mi = $state(0);
  $effect(() => {
    void unitId;
    mi = 0;
  });
  const month = $derived(unit?.months[Math.min(mi, (unit?.months.length ?? 1) - 1)] ?? null);
  const batch = $derived(unit?.batches.find((b) => b.period === month?.period) ?? null);
  let busy = $state(false);
  async function step(id: string) {
    if (!batch) return;
    busy = true;
    try {
      await run(api.batchStep(batch.form, batch.unit_id, batch.period, id));
      toast(t('saved'));
      await load();
      onchange();
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }
  const columns = (rows: { values: { label: string }[] }[]) => [...new Set(rows.flatMap((r) => r.values.map((v) => v.label)))];
  const cell = (r: { values: { label: string; value: string }[] }, c: string) => r.values.find((v) => v.label === c)?.value ?? '';
  // Dates as day/month/year; qualifications and roles each on one line.
  const date = (v: string) => (/^\d{4}-\d{2}-\d{2}$/.test(v) ? new Date(v + 'T12:00:00').toLocaleDateString('it') : v);
  const details = (p: Person) => [
    ...p.attributes.map((a) => ({ label: a.label, value: date(a.value) })),
    ...(p.qualifications.length ? [{ label: t('unit_qualifications'), value: p.qualifications.map((q) => q.label).join(', ') }] : []),
    ...(p.roles.length ? [{ label: t('unit_roles'), value: p.roles.map((r) => r.label).join(', ') }] : [])
  ];
</script>

{#if data}
  <div class="stack page">
    <div class="spread head">
      <h1>{unit?.unit_name ?? data.title}</h1>
      {#if data.units.length > 1}
        <select class="pick" aria-label={t('unit')} bind:value={unitId}>
          {#each data.units as u (u.unit_id)}<option value={u.unit_id}>{u.unit_name}</option>{/each}
        </select>
      {/if}
    </div>
    {#if !unit}
      <p class="muted">{t('nothing')}</p>
    {:else}
      <div class="row tabs" role="tablist" aria-label={unit.unit_name}>
        <button class="tab" role="tab" aria-selected={tab === 'members'} class:active={tab === 'members'} onclick={() => (tab = 'members')}>{t('unit_members')} · {unit.members.length}</button>
        <button class="tab" role="tab" aria-selected={tab === 'reports'} class:active={tab === 'reports'} onclick={() => (tab = 'reports')}>{t('unit_reports')}</button>
      </div>

      {#if tab === 'members'}
        <section class="card list">
          {#each unit.members as p (p.person_id)}
            <div class="member">
              <button class="row-btn" aria-expanded={open === p.person_id} onclick={() => (open = open === p.person_id ? null : p.person_id)}>
                <span class="grow name">{p.name}</span>
                {#each p.marks as m (m)}<span class="badge">{m}</span>{/each}
                <span class="chev" aria-hidden="true">{open === p.person_id ? '▴' : '▾'}</span>
              </button>
              {#if open === p.person_id}
                <dl class="details">
                  {#each details(p) as d, i (i)}<dt>{d.label}</dt><dd>{d.value}</dd>{/each}
                  {#if !details(p).length}<dd class="muted">{t('nothing')}</dd>{/if}
                </dl>
              {/if}
            </div>
          {:else}
            <p class="muted pad">{t('nothing')}</p>
          {/each}
        </section>
      {:else}
        {#if month}
          {@const m = month}
          <section class="card stack">
            <div class="spread">
              <h3 class="cap">{t('unit_summary')}</h3>
              <nav class="months" aria-label={t('period')}>
                <button class="arrow" aria-label={t('previous')} disabled={mi >= unit.months.length - 1} onclick={() => mi++}>‹</button>
                <strong class="cap">{m.period_label}</strong>
                <button class="arrow" aria-label={t('next')} disabled={mi === 0} onclick={() => mi--}>›</button>
              </nav>
            </div>
            <div class="stack tight">
              <strong>{t('unit_received_of').replace('{n}', String(m.received)).replace('{m}', String(m.expected))}</strong>
              <div class="progress" aria-hidden="true"><div style={`width: ${m.expected ? Math.round((m.received / m.expected) * 100) : 0}%`}></div></div>
            </div>
            {#if m.totals.length}<MeasureTable measures={m.totals} />{/if}
          </section>

          <section class="card month">
            <div class="spread box-head">
              <h3>{data.title}</h3>
              {#if batch?.actions.length}
                <div class="row">
                  {#each batch.actions as a (a.id)}<button class="btn primary" disabled={busy || a.disabled} onclick={() => step(a.id)}>{a.label}</button>{/each}
                </div>
              {/if}
            </div>
            {#if batch?.hint}<p class="muted hint">{batch.hint}</p>{/if}
            {#if m.rows.length}
              <div class="scroll">
                <table>
                  <thead><tr>
                    <th><span class="h">{t('name')}</span></th>
                    {#each columns(m.rows) as c (c)}<th title={c}><span class="h">{c}</span></th>{/each}
                    <th><span class="h">{t('state')}</span></th>
                  </tr></thead>
                  <tbody>
                    {#each m.rows as r (r.document_id)}
                      <tr>
                        <td class="who">{r.subject_name}{#if r.variant_label}<span class="muted variant">{r.variant_label}</span>{/if}</td>
                        {#each columns(m.rows) as c (c)}<td>{cell(r, c)}</td>{/each}
                        <td><span class="badge">{r.state_label}</span></td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            {/if}
            {#if batch?.missing.length}
              <div class="missing">
                <h4 class="warn-title">{t('missing')} · {batch.missing.length}</h4>
                {#each batch.missing as p (p.person_id)}
                  <div class="list-row">
                    <span class="grow">{p.name}</span>
                    {#if batch.can_fill_on_behalf}<button class="btn small" onclick={() => go('form', { form: batch.form, subject: p.person_id, period: batch.period })}>{t('fill_on_behalf')}</button>{/if}
                  </div>
                {/each}
              </div>
            {:else if !m.rows.length}
              <p class="muted hint">{t('reports_none')}</p>
            {/if}
          </section>
        {/if}
      {/if}
    {/if}
  </div>
{/if}

<style>
  .page { gap: 18px; }
  .head { align-items: center; }
  .head h1 { margin: 0; }
  .pick { width: auto; min-width: 180px; }
  .tabs { gap: 6px; }
  .tab { border: 1px solid var(--border); background: var(--surface); color: var(--text); border-radius: 999px; padding: 6px 14px; font: inherit; font-size: 14px; cursor: pointer; }
  .tab.active { background: var(--primary-soft); color: var(--primary-strong); border-color: transparent; font-weight: 600; }
  .list { padding: 4px 0; }
  .member + .member { border-top: 1px solid var(--border); }
  .row-btn { display: flex; align-items: center; gap: 8px; width: 100%; padding: 12px 20px; background: none; border: none; font: inherit; color: var(--text); text-align: left; cursor: pointer; }
  .row-btn:hover { background: var(--primary-soft); }
  .name { font-weight: 500; }
  .chev { color: var(--muted); }
  .details { display: grid; grid-template-columns: minmax(120px, max-content) 1fr; gap: 6px 16px; margin: 0; padding: 4px 20px 16px; font-size: 14px; }
  .details dt { color: var(--muted); }
  .details dd { margin: 0; }
  .pad { padding: 12px 20px; }
  .cap { margin: 0; }
  .cap::first-letter { text-transform: uppercase; }
  .tight { gap: 6px; }
  .months { display: flex; align-items: center; gap: 8px; }
  .months strong { font-size: 16px; min-width: 130px; text-align: center; }
  .arrow { flex: none; width: 32px; height: 40px; border-radius: 8px; border: none; background: none; color: var(--primary); font-size: 32px; line-height: 1; cursor: pointer; display: grid; place-items: center; padding: 0 0 4px; }
  .arrow:hover:not(:disabled) { color: var(--primary-strong); background: var(--primary-soft); }
  .arrow:disabled { color: var(--border); cursor: default; }
  .box-head { padding: 0 20px 8px; align-items: center; }
  .box-head h3 { margin: 0; }
  .hint { margin: 0; padding: 0 20px 8px; font-size: 14px; }
  .missing { padding: 8px 20px 12px; }
  .missing h4 { margin: 8px 0; font-size: 15px; }
  .warn-title { color: var(--warn); }
  .month { padding: 16px 0 6px; }
  .scroll { overflow-x: auto; }
  .month table { border-collapse: collapse; width: 100%; font-size: 15px; }
  .month th { text-align: left; font-size: 13px; font-weight: 600; color: var(--muted); padding: 8px 12px; border-bottom: 1px solid var(--border); white-space: nowrap; }
  .month td { padding: 10px 12px; border-bottom: 1px solid var(--border); white-space: nowrap; }
  .month th:first-child, .month td:first-child { padding-left: 20px; }
  .month tr:last-child td { border-bottom: none; }
  .who { font-weight: 500; }
  .variant { display: block; font-weight: 400; font-size: 12px; }
  /* Phone: headers do not widen the columns (as in the previous documents). */
  @media (max-width: 760px) {
    .month th, .month td { padding-left: 8px; padding-right: 8px; }
    .month th:first-child, .month td:first-child { padding-left: 16px; }
    .month th { min-width: 44px; }
    .month th .h { display: block; width: 0; min-width: 100%; overflow: hidden; text-overflow: ellipsis; }
  }
</style>
