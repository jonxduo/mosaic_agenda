<script lang="ts">
  // The units one oversees (e.g. a group): its members, read only, and its
  // documents month by month, the period being collected on top.
  import type { UnitReports_Serialize as Data, PersonView_Serialize as Person } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail } from '../toast.svelte';
  import BatchCard from '../components/BatchCard.svelte';

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
  // The collection card below already counts who handed in for that month.
  const collecting = $derived(!!unit?.summary && unit.batches.some((b) => b.period_label === unit.summary?.period_label));
  const fmt = (n: number | null) => (n === null ? '—' : Number.isInteger(n) ? String(n) : n.toLocaleString('it', { maximumFractionDigits: 1 }));
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
        {#if unit.summary}
          {@const s = unit.summary}
          <section class="card stack">
            <div class="spread">
              <h3 class="cap">{t('unit_summary')} · {s.period_label}</h3>
              {#if !collecting}<strong>{t('unit_received_of').replace('{n}', String(s.received)).replace('{m}', String(s.expected))}</strong>{/if}
            </div>
            {#if !collecting}
              <div class="progress" aria-hidden="true"><div style={`width: ${s.expected ? Math.round((s.received / s.expected) * 100) : 0}%`}></div></div>
            {/if}
            {#if s.totals.length}
              <table class="totals"><tbody>
                {#each s.totals as m (m.label)}<tr><td>{m.label}</td><td class="num">{fmt(m.value)}</td></tr>{/each}
              </tbody></table>
            {/if}
          </section>
        {/if}
        {#each unit.batches as b (b.period)}
          <BatchCard {b} showUnit={false} ondone={() => { load(); onchange(); }} />
        {/each}
        {#each unit.months as m (m.period)}
          <section class="card month">
            <h3 class="cap">{m.period_label}</h3>
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
          </section>
        {/each}
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
  .totals td { padding: 8px 0; }
  .totals .num { text-align: right; font-variant-numeric: tabular-nums; }
  .month { padding: 16px 0 6px; }
  .month h3 { padding: 0 20px 8px; }
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
