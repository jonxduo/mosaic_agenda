<script lang="ts">
  import type { ResourceDetail_Serialize as Detail, ResourceList_Serialize as List } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import FieldInput from '../components/FieldInput.svelte';
  import { fail, toast } from '../toast.svelte';
  import Picture from '../components/Picture.svelte';
  import { scanCard } from '../scan';

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

  // Editing the attributes (who manages the resource).
  let editing = $state(false);
  let edit = $state<Record<string, unknown>>({});
  let formKey = $state(0);
  let scanning = $state(false);
  function startEdit() {
    if (!r) return;
    edit = Object.fromEntries(r.fields.map((f) => [f.id, f.value]));
    editing = true;
  }
  async function scan(files: FileList | null) {
    const file = files?.[0];
    if (!file || !r) return;
    scanning = true;
    try {
      const type = await run(api.menu()).then((m) => m.find((x) => x.view === 'resource_list')?.params.resource_type);
      if (type) {
        edit = { ...edit, ...(await scanCard(type, file)) };
        formKey++;
      }
    } catch (e) {
      fail(e);
    } finally {
      scanning = false;
    }
  }
  async function saveEdit() {
    await act(() => run(api.updateResource(resource, edit)));
    editing = false;
  }

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
    <div class="title">
      <h1>{r.title}</h1>
      {#if r.can_manage && !editing}<button class="btn" onclick={startEdit}>{t('edit')}</button>{/if}
    </div>
    {#if editing}
      <section class="card stack">
        {#if r.can_scan}
          <label class="btn scan" class:busy={scanning}>
            <input type="file" accept="image/*" hidden disabled={scanning} onchange={(e) => scan(e.currentTarget.files)} />
            {scanning ? t('scanning') : t('scan_card')}
          </label>
        {/if}
        {#key formKey}
          {#each r.fields as f (f.id)}<FieldInput field={f} bind:value={edit[f.id]} />{/each}
        {/key}
        <div class="row">
          <button class="btn primary" onclick={saveEdit}>{t('save_plain')}</button>
          <button class="btn" onclick={() => (editing = false)}>{t('cancel')}</button>
        </div>
      </section>
    {:else}
      {#each r.pictures as p (p.reference)}<Picture value={p.reference} alt={p.label} />{/each}
      <table><tbody>
        {#each r.attributes as a (a.label)}<tr><td class="muted">{a.label}</td><td class="pre">{a.value}</td></tr>{/each}
        {#each r.places as p (p.link)}
          <tr><td class="muted">{p.label}</td><td><a href={p.link} target="_blank" rel="noopener noreferrer">{(p.lat ?? 0).toFixed(5)}, {(p.lng ?? 0).toFixed(5)}</a> · <a href={p.link} target="_blank" rel="noopener noreferrer">{t('open_map')}</a></td></tr>
        {/each}
      </tbody></table>
    {/if}
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
  .title { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .title h1 { margin: 0; }
  .scan { align-self: flex-start; cursor: pointer; }
  .busy { opacity: 0.6; pointer-events: none; }
  .pre { white-space: pre-line; }
</style>
