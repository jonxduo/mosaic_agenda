<script lang="ts">
  import type { WorkQueue_Serialize as Queue } from '../bindings';
  import { api, run } from '../api';
  import { fail } from '../toast.svelte';
  import BatchCard from '../components/BatchCard.svelte';

  let { workflow, unitType, onchange }: { workflow: string; unitType?: string; onchange: () => void } = $props();
  let q = $state<Queue | null>(null);

  async function load() {
    try {
      q = await run(api.workQueue(workflow, unitType ?? null));
    } catch (e) {
      fail(e);
    }
  }
  load();
</script>

{#if q}
  <div class="stack page">
    <h1>{q.title}</h1>
    {#each q.batches as b (b.unit_id + b.period)}
      <BatchCard {b} ondone={() => { load(); onchange(); }} />
    {/each}
  </div>
{/if}

<style>
  .page { gap: 24px; }
</style>
