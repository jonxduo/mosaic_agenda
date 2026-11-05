<script lang="ts" module>
  // The month last looked at, kept when coming back from a program.
  let remembered: string | null = null;
</script>

<script lang="ts">
  // The board as tiles (icon, title, month), one month at a time (old
  // programs included); a click opens the content.
  import type { BoardItem } from '../bindings';
  import { api, run } from '../api';
  import { fail } from '../toast.svelte';
  import Icon from '../components/Icon.svelte';
  import { go } from '../router.svelte';
  import { t } from '../i18n';
  let items = $state<BoardItem[]>([]);
  let loaded = $state(false);
  const thisMonth = () => new Date().toLocaleDateString('sv').slice(0, 7);
  let month = $state(remembered ?? thisMonth());
  $effect(() => {
    remembered = month;
    loaded = false;
    run(api.board(month))
      .then((x) => (items = x))
      .catch(fail)
      .finally(() => (loaded = true));
  });
  function shift(n: number) {
    const [y, m] = month.split('-').map(Number);
    const d = new Date(y, m - 1 + n, 1);
    month = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`;
  }
  const label = $derived.by(() => {
    const [y, m] = month.split('-').map(Number);
    const s = new Date(y, m - 1, 1).toLocaleDateString('it', { month: 'long', year: 'numeric' });
    return s.charAt(0).toUpperCase() + s.slice(1);
  });
</script>

<div class="wrap stack">
<nav class="months" aria-label={t('month')}>
  <button class="arrow" aria-label={t('previous')} onclick={() => shift(-1)}>‹</button>
  <span class="sep" aria-hidden="true"></span>
  <strong>{label}</strong>
  <span class="sep" aria-hidden="true"></span>
  <button class="arrow" aria-label={t('next')} onclick={() => shift(1)}>›</button>
</nav>
{#if loaded && !items.length}<p class="muted empty">{t('board_empty')}</p>{/if}
<div class="tiles">
  {#each items as b (b.id)}
    <button class="card tile" onclick={() => go('board_post', { post: b.id, month })}>
      <span class="icon"><Icon name={b.icon ?? 'notice'} size={44} /></span>
      <strong>{b.title}</strong>
      {#if b.subtitle}<span class="muted when">{b.subtitle}</span>{/if}
    </button>
  {/each}
</div>

</div>

<style>
  .wrap { gap: 16px; }
  /* No background: just the month and its arrows. */
  .months { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .months strong { font-size: 18px; text-align: center; flex: 1; }
  /* Arrows as the back button: purple, no border, a line apart from the month. */
  .arrow { flex: none; width: 32px; height: 40px; border-radius: 8px; border: none; background: none; color: var(--primary); font-size: 32px; line-height: 1; cursor: pointer; display: grid; place-items: center; padding: 0 0 4px; }
  .arrow:hover { color: var(--primary-strong); background: var(--primary-soft); }
  .sep { flex: none; align-self: stretch; width: 1px; min-height: 32px; background: var(--border); }
  .empty { text-align: center; }
  .tiles { display: grid; grid-template-columns: repeat(auto-fill, minmax(170px, 1fr)); gap: 14px; }
  .tile { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px; text-align: center; min-height: 170px; color: var(--text); cursor: pointer; font: inherit; }
  .tile:hover { border-color: var(--primary); box-shadow: 0 0 0 3px var(--primary-soft); }
  .tile .icon { color: var(--primary); display: grid; place-items: center; width: 72px; height: 72px; border-radius: 18px; background: var(--primary-soft); margin-bottom: 4px; }
  .tile strong { font-size: 16px; }
  .when { font-size: 13px; text-transform: capitalize; }
</style>
