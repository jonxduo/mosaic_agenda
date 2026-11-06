<script lang="ts">
  import { ask } from '../ask.svelte';
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

  // Assignments are accepted unless one says otherwise: they live under
  // "In programma", not among the things to do.
  const todo = $derived(home?.sections.filter((s) => s.kind !== 'board' && s.kind !== 'work' && s.kind !== 'assignments') ?? []);
  const board = $derived(home?.sections.find((s) => s.kind === 'board'));
  // In "Da fare" only the assignments still to answer; all of them, from the
  // nearest, under "In programma".
  const count = $derived(
    todo.reduce(
      (n, s) =>
        n + (s.kind === 'resources' ? s.items.filter((r) => r.open_request).length : s.items.length),
      0
    )
  );
  const upcoming = $derived(home?.sections.flatMap((s) => (s.kind === 'assignments' ? s.items : [])) ?? []);
  // The calendar square: month on top, the day below; a monthly one, the month.
  const square = (a: { date: string; monthly: boolean }) => {
    const d = new Date(`${a.date}T12:00:00`);
    const month = d.toLocaleDateString('it', { month: 'short' }).replace('.', '');
    return a.monthly
      ? { top: String(d.getFullYear()), main: month.charAt(0).toUpperCase() + month.slice(1) }
      : { top: month.toUpperCase(), main: String(d.getDate()) };
  };
  const work = $derived(home?.sections.find((s) => s.kind === 'work'));

  // "Non posso": the only answer, after asking.
  async function cannot(a: AssignmentItem) {
    if (!(await ask(t('cannot_confirm').replace('{what}', `${a.title} · ${a.subtitle}`), t('decline'), true))) return;
    try {
      await run(api.respond(a.program_id, a.program_type, a.occurrence, a.slot, a.index, false, null, null));
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
      {#if count === 0 && work?.kind === 'work' && work.items.length}
        <p class="muted">{t('nothing_mine')}</p>
      {:else if count === 0}
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

    {#if upcoming.length}
      <section class="stack" aria-labelledby="upcoming">
        <h2 id="upcoming">{t('upcoming')} · {upcoming.length}</h2>
        {#each upcoming as a (a.program_id + a.slot + a.occurrence + a.index)}
          {@const q = square(a)}
          <div class="card spread act">
            <div class="cal" class:month={a.monthly} aria-hidden="true"><span class="top">{q.top}</span><span class="main">{q.main}</span></div>
            <div class="stack tight grow"><h3>{a.title}</h3><span class="muted">{a.subtitle}</span></div>
            {#if a.response === 'declined'}<span class="badge warn">{t('response_declined')}</span>
            {:else if a.can_respond}<button class="btn small" onclick={() => cannot(a)}>{t('decline')}</button>{/if}
          </div>
        {/each}
      </section>
    {/if}

    {#if work && work.kind === 'work' && work.items.length}
      <section class="stack" aria-labelledby="work">
        <h2 id="work">{t('work_for_others')} · {work.items.length}</h2>
        {#each work.items as w, i (i)}
          <div class="card spread">
            <div class="stack tight grow"><h3>{w.title}</h3><span class:warn={w.urgent} class:muted={!w.urgent}>{w.subtitle}</span></div>
            <button class="btn" class:primary={w.urgent} onclick={() => go(w.view, w.params)}>{w.action}</button>
          </div>
        {/each}
      </section>
    {/if}

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
  /* Activity: a calendar page on the left (month band, day below). */
  .act { gap: 14px; align-items: center; flex-wrap: nowrap; }
  .cal { flex: none; display: flex; flex-direction: column; width: 54px; border-radius: 10px; overflow: hidden; border: 1px solid var(--border); background: var(--bg); text-align: center; }
  .cal .top { background: var(--primary); color: #fff; font-size: 11px; font-weight: 700; letter-spacing: 0.04em; padding: 2px 0; }
  .cal .main { font-size: 22px; font-weight: 700; line-height: 1; padding: 6px 0 8px; font-variant-numeric: tabular-nums; }
  .cal.month .main { font-size: 16px; padding: 9px 0 10px; }
  .tight { gap: 4px; }
  .link { color: var(--text); text-decoration: none; }
</style>
