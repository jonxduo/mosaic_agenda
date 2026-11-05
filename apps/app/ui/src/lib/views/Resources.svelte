<script lang="ts">
  import type { ResourceList_Serialize as List } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { go } from '../router.svelte';
  import FieldInput from '../components/FieldInput.svelte';
  import { fail } from '../toast.svelte';
  import { scanCard } from '../scan';

  let { resourceType }: { resourceType: string } = $props();
  let list = $state<List | null>(null);
  let values = $state<Record<string, unknown>>({});
  async function load() {
    list = await run(api.resources(resourceType)).catch((e) => { fail(e); return null; });
  }
  load();
  let scanning = $state(false);
  // Remounts the inputs after a scan: some keep local state.
  let formKey = $state(0);
  async function scan(files: FileList | null) {
    const file = files?.[0];
    if (!file) return;
    scanning = true;
    try {
      values = { ...values, ...(await scanCard(resourceType, file)) };
      formKey++;
    } catch (e) {
      fail(e);
    } finally {
      scanning = false;
    }
  }
  async function create() {
    try {
      await run(api.createResource(resourceType, values));
      values = {};
      await load();
    } catch (e) {
      fail(e);
    }
  }
</script>

{#if list}
  <div class="stack page">
    <h1>{list.title}</h1>
    <table>
      <tbody>
        {#each list.resources as r (r.resource_id)}
          <tr>
            <td><a href="#/resource_detail" onclick={(e) => { e.preventDefault(); go('resource_detail', { resource: r.resource_id }); }}>{r.title}</a></td>
            <td>{r.holder ?? ''}</td>
            <td><span class="badge" class:ok={r.status === 'available'}>{t(r.status as never)}</span></td>
          </tr>
        {/each}
      </tbody>
    </table>
    {#if list.can_manage}
      <section class="card stack">
        <div class="head">
          <h3>{t('new_resource')}</h3>
          {#if list.can_scan}
            <label class="btn" class:busy={scanning}>
              <input type="file" accept="image/*" hidden disabled={scanning} onchange={(e) => scan(e.currentTarget.files)} />
              {scanning ? t('scanning') : t('scan_card')}
            </label>
          {/if}
        </div>
        {#key formKey}
          {#each list.attributes as f (f.id)}<FieldInput field={f} bind:value={values[f.id]} />{/each}
        {/key}
        <button class="btn primary" onclick={create}>{t('add')}</button>
      </section>
    {/if}
  </div>
{/if}

<style>
  .page { gap: 18px; }
  .head { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  .head h3 { margin: 0; }
  .head .btn { cursor: pointer; }
  .busy { opacity: 0.6; pointer-events: none; }
</style>
