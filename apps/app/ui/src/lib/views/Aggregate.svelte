<script lang="ts">
  import MeasureTable from '../components/MeasureTable.svelte';
  import type { AggregateView_Serialize as Agg } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { go } from '../router.svelte';
  import { fail, toast } from '../toast.svelte';

  let { aggregate, period }: { aggregate: string; period?: string } = $props();
  let v = $state<Agg | null>(null);
  let busy = $state(false);

  async function load() {
    try {
      v = await run(api.aggregate(aggregate, period ?? null));
    } catch (e) {
      fail(e);
    }
  }
  load();

  async function doit(kind: 'generate' | 'distribute') {
    if (!v) return;
    busy = true;
    try {
      v = kind === 'generate' ? await run(api.generateAggregate(aggregate, v.period, null)) : await run(api.distributeAggregate(aggregate, v.period));
      toast(t('saved'));
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }
  /** The month before or after (`YYYY-MM`). */
  function shift(by: number) {
    if (!v) return;
    const [y, m] = v.period.split('-').map(Number);
    const d = new Date(y, m - 1 + by, 1);
    go('aggregate', { aggregate, period: `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}` });
  }
</script>

{#if v}
  <div class="stack page">
    <div class="spread">
      <h1>{v.title}</h1>
      <!-- The month, moved with the arrows as in the board. -->
      <nav class="months" aria-label={t('period')}>
        <button class="arrow" aria-label={t('previous')} onclick={() => shift(-1)}>‹</button>
        <strong>{v.period_label}</strong>
        <button class="arrow" aria-label={t('next')} onclick={() => shift(1)}>›</button>
      </nav>
    </div>
    <div class="row">
      {#if v.version}<span class="badge">{t('version')} {v.version}</span>{/if}
      <span class="badge" class:ok={v.distributed}>{v.distributed ? t('distributed') : t('not_distributed')}</span>
      <span class="muted">{t('registered')}: {v.registered_count}</span>
    </div>
    <div class="row">
      {#if v.can_generate}<button class="btn primary" disabled={busy} onclick={() => doit('generate')}>{t('generate')}</button>{/if}
      {#if v.can_distribute && !v.distributed}<button class="btn" disabled={busy} onclick={() => doit('distribute')}>{t('distribute')}</button>{/if}
    </div>
    {#if !v.groups.length}<p class="muted">{t('none_visible')}</p>{/if}
    {#each v.groups as g (g.label)}
      <section class="card">
        <h3>{g.label}</h3>
        <MeasureTable measures={g.measures} />
      </section>
    {/each}
    {#if v.missing.length}
      <section class="card stack">
        <h2>{t('missing')} · {v.missing.length}</h2>
        <p>{v.missing.join(', ')}</p>
      </section>
    {/if}
  </div>
{/if}

<style>
  .page { gap: 20px; }
  .months { display: flex; align-items: center; gap: 8px; }
  .months strong { font-size: 18px; min-width: 150px; text-align: center; }
  .months strong::first-letter { text-transform: uppercase; }
  .arrow { flex: none; width: 32px; height: 40px; border-radius: 8px; border: none; background: none; color: var(--primary); font-size: 32px; line-height: 1; cursor: pointer; display: grid; place-items: center; padding: 0 0 4px; }
  .arrow:hover { color: var(--primary-strong); background: var(--primary-soft); }
</style>
