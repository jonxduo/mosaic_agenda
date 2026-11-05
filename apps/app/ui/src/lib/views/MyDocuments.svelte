<script lang="ts">
  // My reports: the next one to fill always on top, the ones before in a table.
  import type { DocumentRow_Serialize as Row } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail } from '../toast.svelte';
  import Form from './Form.svelte';

  let { form }: { form?: string } = $props();
  let rows = $state<Row[]>([]);
  let next = $state<string | null>(null);
  let loaded = $state(false);

  async function load() {
    try {
      rows = await run(api.myDocuments(form ?? null));
      next = form ? await run(api.nextPeriod(form)) : null;
    } catch (e) {
      fail(e);
    } finally {
      loaded = true;
    }
  }
  load();

  const past = $derived(rows.filter((r) => r.period !== next));
  // Columns: the answers' labels, in the order they first appear.
  const columns = $derived([...new Set(past.flatMap((r) => r.values.map((v) => v.label)))]);
  const cell = (r: Row, label: string) => r.values.find((v) => v.label === label)?.value ?? '';
</script>

<div class="stack page">
  {#if form && next}
    {#key next}
      <section class="card next">
        <Form {form} period={next} embedded onchange={load} />
      </section>
    {/key}
  {/if}

  {#if past.length}
    <section class="card table-card">
      <h3>{t('reports_previous')}</h3>
      <div class="scroll">
        <table>
          <thead>
            <tr>
              <th title={t('report_period')}><span class="h">{t('report_period')}</span></th>
              {#each columns as c (c)}<th title={c}><span class="h">{c}</span></th>{/each}
              <th title={t('state')}><span class="h">{t('state')}</span></th>
            </tr>
          </thead>
          <tbody>
            {#each past as r (r.document_id)}
              <tr>
                <td class="period">{r.period_label}{#if r.variant_label}<span class="muted variant">{r.variant_label}</span>{/if}</td>
                {#each columns as c (c)}<td>{cell(r, c)}</td>{/each}
                <td><span class="badge state" class:ok={!r.editable} title={r.state_label}>{r.state_label}</span></td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>
  {:else if loaded && !next}
    <p class="muted">{t('reports_none')}</p>
  {/if}
</div>

<style>
  .page { gap: 18px; }
  .next :global(.page) { gap: 12px; }
  .table-card { padding: 16px 0 6px; }
  .table-card h3 { padding: 0 20px 8px; }
  .scroll { overflow-x: auto; }
  table { border-collapse: collapse; width: 100%; font-size: 15px; }
  th { text-align: left; font-size: 13px; font-weight: 600; color: var(--muted); padding: 8px 12px; border-bottom: 1px solid var(--border); white-space: nowrap; }
  td { padding: 10px 12px; border-bottom: 1px solid var(--border); white-space: nowrap; }
  th:first-child, td:first-child { padding-left: 20px; }
  tr:last-child td { border-bottom: none; }
  .period { font-weight: 600; text-transform: capitalize; }
  /* Phone: the headers do not widen the columns; a long one ends in "…"
     (full text on hover). The table scrolls only if the values do not fit. */
  @media (max-width: 760px) {
    th, td { padding-left: 8px; padding-right: 8px; }
    th:first-child, td:first-child { padding-left: 16px; }
    th { min-width: 44px; }
    .period { white-space: normal; }
    .state { display: inline-block; max-width: 96px; overflow: hidden; text-overflow: ellipsis; vertical-align: middle; }
    th .h { display: block; width: 0; min-width: 100%; overflow: hidden; text-overflow: ellipsis; }
  }
  .variant { display: block; font-weight: 400; font-size: 12px; text-transform: none; }
</style>
