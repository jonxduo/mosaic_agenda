<script lang="ts">
  import type { HeldResource_Serialize as Held } from '../bindings';
  import { api, run } from '../api';
  import { go } from '../router.svelte';
  import { fail } from '../toast.svelte';
  let { resourceType }: { resourceType?: string } = $props();
  let items = $state<Held[]>([]);
  run(api.myResources(resourceType ?? null)).then((x) => (items = x)).catch(fail);
</script>

<div class="stack">
  {#each items as r (r.resource_id)}
    <a class="card spread link" href="#/resource_detail" onclick={(e) => { e.preventDefault(); go('resource_detail', { resource: r.resource_id }); }}>
      <div class="stack tight grow"><h3>{r.title}</h3><span class="muted">{r.subtitle}</span></div>
      {#if r.open_request}<span class="warn">{r.open_request.label}</span>{/if}
    </a>
  {/each}
</div>

<style>
  .tight { gap: 4px; }
  .link { color: var(--text); text-decoration: none; }
</style>
