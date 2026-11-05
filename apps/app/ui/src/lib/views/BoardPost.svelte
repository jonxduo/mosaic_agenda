<script lang="ts">
  // One post of the board in its own page, with a way back to the board.
  import type { BoardItem } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { go } from '../router.svelte';
  import { fail } from '../toast.svelte';
  import PageHeading from '../components/PageHeading.svelte';
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
  <header>
    <PageHeading title={item?.title ?? ''} subtitle={item?.subtitle ?? ''} onback={() => go('board')} />
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
  table { border-collapse: collapse; width: 100%; }
  td { padding: 8px 10px 8px 0; border-bottom: 1px solid var(--border); vertical-align: top; }
  tr:last-child td { border-bottom: none; }
</style>
