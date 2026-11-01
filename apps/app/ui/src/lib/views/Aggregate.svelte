<script lang="ts">
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
  function month(e: Event) {
    const value = (e.currentTarget as HTMLInputElement).value;
    if (value) go('aggregate', { aggregate, period: value });
  }
</script>

{#if v}
  <div class="stack page">
    <div class="spread">
      <div class="stack tight"><h1>{v.title}</h1><p class="muted">{v.period_label}</p></div>
      <input type="month" aria-label={t('period')} value={v.period} onchange={month} style="width: 180px" />
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
        <table><tbody>
          {#each g.measures as m (m.label)}<tr><td>{m.label}</td><td class="num">{Number.isInteger(m.value ?? 0) ? (m.value ?? 0) : (m.value ?? 0).toFixed(1)}</td></tr>{/each}
        </tbody></table>
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
  .tight { gap: 4px; }
</style>
