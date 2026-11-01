<script lang="ts">
  import type { ResourceDetail_Serialize as Detail, ResourceList_Serialize as List } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import FieldInput from '../components/FieldInput.svelte';
  import { fail, toast } from '../toast.svelte';

  let { resource }: { resource: string } = $props();
  let r = $state<Detail | null>(null);
  let list = $state<List | null>(null);
  let holder = $state('');
  let request = $state('');
  let values = $state<Record<string, unknown>>({});

  async function load() {
    try {
      r = await run(api.resource(resource));
    } catch (e) {
      fail(e);
    }
  }
  load();

  async function withList() {
    if (!r || list) return;
    const types = await run(api.menu()).then((m) => m.find((x) => x.view === 'resource_list')?.params.resource_type).catch(() => undefined);
    if (types) list = await run(api.resources(types)).catch(() => null);
  }
  $effect(() => { if (r?.can_assign || r?.can_request) withList(); });

  async function act(f: () => Promise<unknown>) {
    try {
      await f();
      toast(t('saved'));
      values = {};
      await load();
    } catch (e) {
      fail(e);
    }
  }
</script>

{#if r}
  <div class="stack page">
    <h1>{r.title}</h1>
    <table><tbody>{#each r.attributes as a (a.label)}<tr><td class="muted">{a.label}</td><td>{a.value}</td></tr>{/each}</tbody></table>
    <p><strong>{t('holder')}:</strong> {r.holder ?? t('available')}</p>

    {#if r.open_request && r.is_holder}
      <section class="card stack">
        <h3>{r.open_request.label}</h3>
        <p class="muted">{r.open_request.due}</p>
        <div class="row">
          {#each r.open_request.answers as a (a.id)}<button class="btn" onclick={() => act(() => run(api.answerRequest(r!.open_request!.request_id, a.id)))}>{a.label}</button>{/each}
        </div>
      </section>
    {/if}

    {#if r.is_holder}
      <section class="card stack">
        {#each r.log_fields as f (f.id)}<FieldInput field={f} bind:value={values[f.id]} />{/each}
        <div class="row">
          <button class="btn primary" onclick={() => act(() => run(api.logResource(resource, values)))}>{t('log')}</button>
          <button class="btn" onclick={() => act(() => run(api.returnResource(resource)))}>{t('return')}</button>
        </div>
      </section>
    {/if}

    {#if r.can_assign && !r.holder && list}
      <div class="row">
        <select aria-label={t('holder')} bind:value={holder} style="max-width: 280px">
          <option value=""></option>
          {#each list.candidates as c (c.id)}<option value={c.id}>{c.label}</option>{/each}
        </select>
        <button class="btn primary" disabled={!holder} onclick={() => act(() => run(api.assignResource(resource, holder)))}>{t('assign')}</button>
      </div>
    {/if}
    {#if r.can_request && list && !r.open_request}
      <div class="row">
        <select aria-label={t('request')} bind:value={request} style="max-width: 280px">
          <option value=""></option>
          {#each list.requests as q (q.id)}<option value={q.id}>{q.label}</option>{/each}
        </select>
        <button class="btn" disabled={!request} onclick={() => act(() => run(api.openRequest(resource, request)))}>{t('request')}</button>
        {#if r.can_assign}<button class="btn" onclick={() => act(() => run(api.returnResource(resource)))}>{t('return')}</button>{/if}
      </div>
    {/if}

    {#if r.history.length}
      <section class="stack">
        <h2>{t('history')}</h2>
        {#each r.history as h, i (i)}<div class="list-row"><span class="grow">{h.label}</span><span class="muted">{h.date}</span></div>{/each}
      </section>
    {/if}
  </div>
{/if}

<style>
  .page { gap: 18px; }
</style>
