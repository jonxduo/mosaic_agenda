<script lang="ts">
  // One post of the board in its own page, with a way back to the board.
  import type { BoardItem } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { go } from '../router.svelte';
  import { fail } from '../toast.svelte';
  import Icon from '../components/Icon.svelte';
  import Program from './Program.svelte';

  let { post }: { post: string } = $props();
  let item = $state<BoardItem | null>(null);
  let missing = $state(false);
  run(api.board())
    .then((x) => {
      item = x.find((b) => b.id === post) ?? null;
      missing = !item;
    })
    .catch(fail);
</script>

<div class="stack page wide">
  <header class="row head">
    <button class="back" aria-label={t('back')} title={t('back')} onclick={() => go('board')}>‹</button>
    {#if item}
      <span class="icon"><Icon name={item.icon ?? 'notice'} size={26} /></span>
      <div class="stack tight">
        <h1>{item.title}</h1>
        {#if item.subtitle}<span class="muted when">{item.subtitle}</span>{/if}
      </div>
    {/if}
  </header>
  {#if item?.icon}
    <!-- A program: the same layout as its editor, as text. -->
    <Program program={post} readonly />
  {:else if item}
    <article class="card">
      <table><tbody>
        {#each item.body as l, i (i)}<tr>{#if l.label}<td class="muted">{l.label}</td>{/if}<td>{l.value}</td></tr>{/each}
      </tbody></table>
    </article>
  {:else if missing}
    <p class="muted">{t('board_post_gone')}</p>
  {/if}
</div>

<style>
  .page { gap: 16px; }
  .tight { gap: 2px; }
  .head { gap: 12px; align-items: center; flex-wrap: nowrap; }
  .back { flex: none; width: 40px; height: 40px; border-radius: 12px; border: 1px solid var(--border); background: var(--bg); color: var(--text); font-size: 24px; line-height: 1; cursor: pointer; display: grid; place-items: center; padding: 0 0 3px; }
  .back:hover { border-color: var(--primary); color: var(--primary); }
  .icon { flex: none; color: var(--primary); display: grid; place-items: center; width: 44px; height: 44px; border-radius: 12px; background: var(--primary-soft); }
  .when { font-size: 14px; text-transform: capitalize; }
  table { border-collapse: collapse; width: 100%; }
  td { padding: 8px 10px 8px 0; border-bottom: 1px solid var(--border); vertical-align: top; }
  tr:last-child td { border-bottom: none; }
</style>
