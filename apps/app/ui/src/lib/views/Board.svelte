<script lang="ts">
  import type { BoardItem } from '../bindings';
  import { api, run } from '../api';
  import { fail } from '../toast.svelte';
  let items = $state<BoardItem[]>([]);
  run(api.board()).then((x) => (items = x)).catch(fail);
</script>

<div class="stack">
  {#each items as b (b.id)}
    <article class="card stack">
      <div class="stack tight"><h3>{b.title}</h3>{#if b.subtitle}<span class="muted">{b.subtitle}</span>{/if}</div>
      <table><tbody>
        {#each b.body as l, i (i)}<tr>{#if l.label}<td class="muted">{l.label}</td>{/if}<td>{l.value}</td></tr>{/each}
      </tbody></table>
    </article>
  {/each}
</div>

<style>
  .tight { gap: 4px; }
</style>
