<script lang="ts">
  // The board as tiles (icon, title, month); a click opens the content.
  import type { BoardItem } from '../bindings';
  import { api, run } from '../api';
  import { fail } from '../toast.svelte';
  import Icon from '../components/Icon.svelte';
  import { go } from '../router.svelte';
  let items = $state<BoardItem[]>([]);
  run(api.board()).then((x) => (items = x)).catch(fail);
</script>

<div class="wrap">
<div class="tiles">
  {#each items as b (b.id)}
    <button class="card tile" onclick={() => go('board_post', { post: b.id })}>
      <span class="icon"><Icon name={b.icon ?? 'notice'} size={44} /></span>
      <strong>{b.title}</strong>
      {#if b.subtitle}<span class="muted when">{b.subtitle}</span>{/if}
    </button>
  {/each}
</div>

</div>

<style>
  .tiles { display: grid; grid-template-columns: repeat(auto-fill, minmax(170px, 1fr)); gap: 14px; }
  .tile { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px; text-align: center; min-height: 170px; color: var(--text); cursor: pointer; font: inherit; }
  .tile:hover { border-color: var(--primary); box-shadow: 0 0 0 3px var(--primary-soft); }
  .tile .icon { color: var(--primary); display: grid; place-items: center; width: 72px; height: 72px; border-radius: 18px; background: var(--primary-soft); margin-bottom: 4px; }
  .tile strong { font-size: 16px; }
  .when { font-size: 13px; text-transform: capitalize; }
</style>
