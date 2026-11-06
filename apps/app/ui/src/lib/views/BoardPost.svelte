<script lang="ts">
  // One post of the board in its own page, with a way back to the board.
  import type { BoardItem } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { go } from '../router.svelte';
  import { fail } from '../toast.svelte';
  import PageHeading from '../components/PageHeading.svelte';
  // Names as in the other programs: surname and initials.
  const short = (name: string) => {
    const w = name.trim().split(/\s+/);
    return w.length < 2 ? name : `${w[0]} ${w.slice(1).map((x) => `${x[0]}.`).join(' ')}`;
  };
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
  {#if item?.grid}
    {@const g = item.grid}
    <!-- Programs published together: dates by rows, every assignment of
         every program a column of the same row. -->
    <div class="card grid-card">
      <div class="scroll">
        <table class="grid">
          <thead><tr><th scope="col">{t('dates')}</th>{#each g.columns as c, i (i)}<th scope="col">{c}</th>{/each}</tr></thead>
          <tbody>
            {#each g.rows as r, i (i)}
              <tr>
                <th scope="row" class="when">{#each r.dates as d (d)}<span>{d}</span>{/each}</th>
                {#each r.cells as c, j (j)}<td>{#each c as n (n)}<span>{short(n)}</span>{:else}–{/each}</td>{/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>
  {:else if item?.programs.length}
    <!-- Programs: the same layout as their editor, as text; a group shows
         each of its programs under its name. -->
    {#each item.programs as id (id)}
      <Program program={id} readonly titled={item.programs.length > 1} />
    {/each}
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
  .grid-card { padding: 0; overflow: hidden; }
  .scroll { overflow-x: auto; }
  .grid { font-size: 15px; }
  .grid th, .grid td { padding: 10px 12px; border-bottom: 1px solid var(--border); text-align: left; white-space: nowrap; vertical-align: top; }
  /* One date, one person per line: narrow columns. */
  .grid th span, .grid td span { display: block; }
  .grid thead th { font-size: 13px; font-weight: 600; color: var(--muted); }
  .grid td { font-style: italic; }
  /* The dates stay in view while the columns scroll. */
  .grid .when { position: sticky; left: 0; background: var(--bg); font-weight: 600; }
  .grid thead th:first-child { position: sticky; left: 0; background: var(--bg); }
  .grid tr:last-child th, .grid tr:last-child td { border-bottom: none; }
</style>
