<script lang="ts">
  // One post of the board in its own page, with a way back to the board.
  import type { BoardItem } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { go } from '../router.svelte';
  import { fail } from '../toast.svelte';
  import BackButton from '../components/BackButton.svelte';
  import Program from './Program.svelte';

  let { post, month }: { post: string; month?: string } = $props();
  let item = $state<BoardItem | null>(null);
  let missing = $state(false);
  run(api.board(month ?? null))
    .then((x) => {
      item = x.find((b) => b.id === post) ?? null;
      missing = !item;
    })
    .catch(fail);
</script>

<div class="stack page wide">
  <header class="row head">
    <BackButton onclick={() => go('board')} />
    {#if item}
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
  .when { font-size: 14px; text-transform: capitalize; }
  table { border-collapse: collapse; width: 100%; }
  td { padding: 8px 10px 8px 0; border-bottom: 1px solid var(--border); vertical-align: top; }
  tr:last-child td { border-bottom: none; }
</style>
