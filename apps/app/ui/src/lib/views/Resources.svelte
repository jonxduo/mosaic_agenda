<script lang="ts">
  import type { ResourceList_Serialize as List } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { go } from '../router.svelte';
  import FieldInput from '../components/FieldInput.svelte';
  import { fail } from '../toast.svelte';

  let { resourceType }: { resourceType: string } = $props();
  let list = $state<List | null>(null);
  let values = $state<Record<string, unknown>>({});
  async function load() {
    list = await run(api.resources(resourceType)).catch((e) => { fail(e); return null; });
  }
  load();
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
        <h3>{t('new_resource')}</h3>
        {#each list.attributes as f (f.id)}<FieldInput field={f} bind:value={values[f.id]} />{/each}
        <button class="btn primary" onclick={create}>{t('add')}</button>
      </section>
    {/if}
  </div>
{/if}

<style>
  .page { gap: 18px; }
</style>
