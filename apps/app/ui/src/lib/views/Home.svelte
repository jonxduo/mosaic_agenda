<script lang="ts">
  import type { Home_Serialize as HomeData, Me, AssignmentItem } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { go } from '../router.svelte';
  import { fail, toast } from '../toast.svelte';

  let { me, onchange }: { me: Me | null; onchange: () => void } = $props();
  let home = $state<HomeData | null>(null);

  async function load() {
    try {
      home = await run(api.home());
    } catch (e) {
      fail(e);
    }
  }
  load();

  const todo = $derived(home?.sections.filter((s) => s.kind !== 'board') ?? []);
  const board = $derived(home?.sections.find((s) => s.kind === 'board'));
  const count = $derived(todo.reduce((n, s) => n + (s.kind === 'resources' ? s.items.filter((r) => r.open_request).length : s.items.length), 0));

  async function respond(a: AssignmentItem, accept: boolean) {
    try {
      await run(api.respond(a.program_id, a.program_type, a.occurrence, a.slot, a.index, accept, null, null));
      toast(t('saved'));
      await load();
      onchange();
    } catch (e) {
      fail(e);
    }
  }
</script>

{#if home}
  <div class="stack page">
    <header class="stack head">
      <p class="muted">{home.date}</p>
      <h1>{home.greeting}</h1>
    </header>

    <section class="stack" aria-labelledby="todo">
      <h2 id="todo">{t('todo')} · {count}</h2>
      {#if count === 0}
        <div class="empty">
          <img src="/package/mark.png" alt="" aria-hidden="true" onerror={(e) => e.currentTarget.remove()} />
          <p class="muted">{t('nothing')}</p>
        </div>
      {/if}
      {#each todo as s, i (i)}
        {#if s.kind === 'documents_due'}
          {#each s.items as d (d.form + d.period)}
            <div class="card spread">
              <div class="stack tight grow"><h3>{d.title}</h3><span class:warn={d.urgent} class:muted={!d.urgent}>{d.subtitle}</span></div>
              <button class="btn primary" onclick={() => go('form', { form: d.form, period: d.period })}>{t('fill')}</button>
            </div>
          {/each}
        {:else if s.kind === 'assignments'}
          {#each s.items as a (a.program_id + a.slot + a.occurrence + a.index)}
            <div class="card spread">
              <div class="stack tight grow"><h3>{a.title}</h3><span class="muted">{a.subtitle}</span></div>
              {#if a.can_respond}
                <div class="row">
                  <button class="btn" onclick={() => respond(a, false)}>{t('decline')}</button>
                  <button class="btn primary" onclick={() => respond(a, true)}>{t('accept')}</button>
                </div>
              {:else}<span class="badge">{t(`response_${a.response}` as never)}</span>{/if}
            </div>
          {/each}
        {:else if s.kind === 'resources'}
          {#each s.items.filter((r) => r.open_request) as r (r.resource_id)}
            <div class="card spread">
              <div class="stack tight grow"><h3>{r.title}</h3><span class="muted">{r.open_request?.label} · {r.open_request?.due}</span></div>
              <button class="btn" onclick={() => go('resource_detail', { resource: r.resource_id })}>{t('respond')}</button>
            </div>
          {/each}
        {/if}
      {/each}
    </section>

    {#if board && board.kind === 'board' && board.items.length}
      <section class="stack" aria-labelledby="board">
        <h2 id="board">{t('board_news')}</h2>
        <div>
          {#each board.items as b (b.id)}
            <a class="list-row link" href="#/board" onclick={(e) => { e.preventDefault(); go('board'); }}>
              <span class="grow">{b.title}{b.subtitle ? ` · ${b.subtitle}` : ''}</span>
            </a>
          {/each}
        </div>
      </section>
    {/if}
    {#if me}<span hidden data-testid="me">{me.display_name}</span>{/if}
  </div>
{/if}

<style>
  /* Empty day: the logo, very light, behind the message. */
  .empty { position: relative; display: grid; place-items: center; min-height: 260px; }
  .empty img { position: absolute; width: min(70%, 280px); opacity: 0.07; pointer-events: none; }
  .empty p { position: relative; }
  .page { gap: 36px; }
  .head { gap: 6px; }
  .tight { gap: 4px; }
  .link { color: var(--text); text-decoration: none; }
</style>
