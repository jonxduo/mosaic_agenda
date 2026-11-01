<script lang="ts">
  import type { WorkQueue_Serialize as Queue, BatchView_Serialize as Batch } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { go } from '../router.svelte';
  import { fail, toast } from '../toast.svelte';

  let { workflow, unitType, onchange }: { workflow: string; unitType?: string; onchange: () => void } = $props();
  let q = $state<Queue | null>(null);
  let busy = $state(false);

  async function load() {
    try {
      q = await run(api.workQueue(workflow, unitType ?? null));
    } catch (e) {
      fail(e);
    }
  }
  load();

  async function step(b: Batch, step: string) {
    busy = true;
    try {
      await run(api.batchStep(b.form, b.unit_id, b.period, step));
      toast(t('saved'));
      await load();
      onchange();
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }
  const pct = (b: Batch) => {
    const total = b.received.length + b.missing.length + b.done.length;
    return total ? Math.round(((total - b.missing.length) / total) * 100) : 0;
  };
</script>

{#if q}
  <div class="stack page">
    <h1>{q.title}</h1>
    {#each q.batches as b (b.unit_id + b.period)}
      <section class="card stack batch" data-testid="batch">
        <div class="spread">
          <div class="stack tight"><span class="muted">{b.unit_name}</span><h3>{b.period_label}</h3></div>
          <div class="row">
            {#each b.actions as a (a.id)}<button class="btn primary" disabled={busy || a.disabled} onclick={() => step(b, a.id)}>{a.label}</button>{/each}
          </div>
        </div>
        <div class="stack tight">
          <div class="spread"><strong>{b.progress}</strong>{#if b.hint}<span class="muted">{b.hint}</span>{/if}</div>
          <div class="progress" aria-hidden="true"><div style={`width: ${pct(b)}%`}></div></div>
        </div>
        {#if b.missing.length}
          <div>
            <h2 class="warn-title">{t('missing')} · {b.missing.length}</h2>
            {#each b.missing as p (p.person_id)}
              <div class="list-row">
                <span class="grow">{p.name}</span>
                {#if b.can_fill_on_behalf}<button class="btn small" onclick={() => go('form', { form: b.form, subject: p.person_id, period: b.period })}>{t('fill_on_behalf')}</button>{/if}
              </div>
            {/each}
          </div>
        {/if}
        {#if b.received.length}
          <div>
            <h2>{t('received')} · {b.received.length}</h2>
            {#each b.received as p (p.person_id)}
              <a class="list-row link" href="#/form" onclick={(e) => { e.preventDefault(); go('form', { form: b.form, subject: p.person_id, period: b.period }); }}>
                <span class="ok" aria-hidden="true">✓</span><span class="grow">{p.name}</span><span class="muted">{p.detail}</span>
              </a>
            {/each}
          </div>
        {/if}
        {#if b.done.length}
          <div>
            <h2>{t('done')} · {b.done.length}</h2>
            {#each b.done as d (d.document_id)}
              <div class="list-row"><span class="grow">{d.subject_name}</span><span class="badge">{d.state_label}</span></div>
            {/each}
          </div>
        {/if}
      </section>
    {/each}
  </div>
{/if}

<style>
  .page { gap: 24px; }
  .batch { gap: 18px; }
  .tight { gap: 6px; }
  .warn-title { color: var(--warn); }
  .link { color: var(--text); text-decoration: none; }
</style>
