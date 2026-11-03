<script lang="ts">
  // The board as tiles (icon, title, month); a click opens the content.
  import type { BoardItem } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail } from '../toast.svelte';
  import Icon from '../components/Icon.svelte';
  let items = $state<BoardItem[]>([]);
  let open = $state<BoardItem | null>(null);
  run(api.board()).then((x) => (items = x)).catch(fail);
</script>

<div class="wrap">
<div class="tiles">
  {#each items as b (b.id)}
    <button class="card tile" onclick={() => (open = b)}>
      <span class="icon"><Icon name={b.icon ?? 'notice'} size={44} /></span>
      <strong>{b.title}</strong>
      {#if b.subtitle}<span class="muted when">{b.subtitle}</span>{/if}
    </button>
  {/each}
</div>

{#if open}
  {@const b = open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="backdrop" onclick={() => (open = null)}></div>
  <div class="card stack modal" role="dialog" aria-modal="true" aria-label={b.title} tabindex="-1" onkeydown={(e) => e.key === 'Escape' && (open = null)}>
    <div class="spread">
      <div class="stack tight"><h3>{b.title}</h3>{#if b.subtitle}<span class="muted when">{b.subtitle}</span>{/if}</div>
      <button class="btn small" onclick={() => (open = null)}>{t('close')}</button>
    </div>
    <table><tbody>
      {#each b.body as l, i (i)}<tr>{#if l.label}<td class="muted">{l.label}</td>{/if}<td>{l.value}</td></tr>{/each}
    </tbody></table>
  </div>
{/if}
</div>

<style>
  .tight { gap: 4px; }
  .tiles { display: grid; grid-template-columns: repeat(auto-fill, minmax(170px, 1fr)); gap: 14px; }
  .tile { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px; text-align: center; min-height: 170px; color: var(--text); cursor: pointer; font: inherit; }
  .tile:hover { border-color: var(--primary); box-shadow: 0 0 0 3px var(--primary-soft); }
  .tile .icon { color: var(--primary); display: grid; place-items: center; width: 72px; height: 72px; border-radius: 18px; background: var(--primary-soft); margin-bottom: 4px; }
  .tile strong { font-size: 16px; }
  .when { font-size: 13px; text-transform: capitalize; }
  .backdrop { position: fixed; inset: 0; background: rgb(0 0 0 / 0.35); z-index: 50; }
  .modal { position: fixed; z-index: 51; left: 50%; top: 50%; transform: translate(-50%, -50%); width: min(640px, calc(100vw - 32px)); max-height: calc(100vh - 64px); overflow-y: auto; box-shadow: 0 12px 40px rgb(0 0 0 / 0.2); }
  table { border-collapse: collapse; width: 100%; }
  td { padding: 6px 8px 6px 0; border-bottom: 1px solid var(--border); vertical-align: top; }
</style>
