<script lang="ts">
  import type { DocumentRow_Serialize as Row } from '../bindings';
  import { api, run } from '../api';
  import { go } from '../router.svelte';
  import { fail } from '../toast.svelte';

  let { form }: { form?: string } = $props();
  let rows = $state<Row[]>([]);
  run(api.myDocuments(form ?? null)).then((r) => (rows = r)).catch(fail);
</script>

<div class="stack">
  {#each rows as r (r.document_id)}
    <a class="card spread link" href="#/form" onclick={(e) => { e.preventDefault(); go('form', { form: r.form, period: r.period }); }}>
      <div class="stack tight grow"><h3>{r.period_label}</h3>{#if r.variant_label}<span class="muted">{r.variant_label}</span>{/if}</div>
      <span class="badge">{r.state_label}</span>
      {#if r.delivery}<span class="muted">{r.delivery}</span>{/if}
    </a>
  {/each}
</div>

<style>
  .tight { gap: 4px; }
  .link { color: var(--text); text-decoration: none; }
</style>
