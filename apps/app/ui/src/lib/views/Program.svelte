<script lang="ts">
  import type { ProgramEditor_Serialize as Editor, SlotCell_Serialize as Cell, SlotChange, DetailChange, ProgramGroup } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail, toast } from '../toast.svelte';

  // One layout for every program: a row per part (the part and its details
  // on the left, who does it on the right, one column per room) and, beside,
  // the people who can take a part this week, the longest without one first.
  let { program }: { program: string } = $props();
  let p = $state<Editor | null>(null);
  let external = $state('');
  let reading = $state(false);

  // Choices and details stay here until "Salva" (or "Invia", which saves first).
  let pending = $state<Record<string, SlotChange>>({});
  let details = $state<Record<string, DetailChange>>({});
  let busy = $state(false);
  const keyOf = (c: { occurrence: number; slot: string; index: number }) => `${c.occurrence}|${c.slot}|${c.index}`;
  const changed = $derived(Object.keys(pending).length + Object.keys(details).length);
  const valueOf = (c: Cell) => pending[keyOf(c)]?.person ?? c.assignee ?? '';
  const text = (v: unknown) => (v === null || v === undefined ? '' : String(v));
  const detailOf = (c: Cell, id: string) => details[keyOf(c)]?.values[id] ?? text(c.details.find((f) => f.id === id)?.value);
  const isRow = (c: Cell) => c.row === c.slot && !c.column;
  const inRow = (top: Cell, column: string | null) =>
    p?.cells.filter((x) => x.occurrence === top.occurrence && x.per_event === top.per_event && x.row === top.slot && x.index === top.index && (x.column ?? null) === column) ?? [];
  const sectionOf = (id: string | null) => p?.sections.find((x) => x.id === id);
  const rowsOf = (occurrence: number, perEvent: boolean) =>
    p?.cells.filter((c) => c.occurrence === occurrence && c.per_event === perEvent && isRow(c)) ?? [];
  const occsOf = (g: ProgramGroup) => new Set([g.occurrence, ...g.days.map((d) => d.occurrence)]);

  // The week (group of events) on screen: the side list is about it.
  let current = $state(0);
  const group = $derived(p?.groups[Math.min(current, (p?.groups.length ?? 1) - 1)]);
  $effect(() => {
    if (!p) return;
    const on = () => {
      let i = 0;
      document.querySelectorAll('section.week').forEach((el, k) => {
        if (el.getBoundingClientRect().top <= 160) i = k;
      });
      current = i;
    };
    on();
    addEventListener('scroll', on, { passive: true });
    return () => removeEventListener('scroll', on);
  });

  // Drag and drop: a person from the side list onto a slot that accepts them.
  let dragging = $state<string | null>(null);
  const accepts = (c: Cell, id: string) => c.candidates.some((o) => o.id === id);
  const nameOf = (c: Cell, id: string) => c.candidates.find((o) => o.id === id)?.label ?? c.assignee_name ?? '';
  const DAY = 86_400_000;
  const dayOf = (iso: string) => Date.UTC(+iso.slice(0, 4), +iso.slice(5, 7) - 1, +iso.slice(8, 10));
  // Who can take a part of this week, by weeks since their last part before
  // it (other programs, then the earlier weeks of this one): never first.
  const people = $derived.by(() => {
    if (!p || !group) return [];
    const occs = occsOf(group);
    const from = dayOf(p.dates[group.occurrence] ?? '');
    const here = p.cells.filter((c) => occs.has(c.occurrence));
    const eligible = new Set(here.flatMap((c) => c.candidates.map((o) => o.id)));
    const taken = new Set(here.map(valueOf).filter(Boolean));
    const before = new Map<string, number>();
    for (const c of p.cells) {
      const v = valueOf(c);
      const d = dayOf(p.dates[c.occurrence] ?? '');
      if (v && d < from && (before.get(v) ?? -Infinity) < d) before.set(v, d);
    }
    return p.people
      .filter((x) => eligible.has(x.id))
      .map((x) => {
        const last = Math.max(x.last ? dayOf(x.last) : -Infinity, before.get(x.id) ?? -Infinity);
        const weeks = taken.has(x.id) ? 0 : last === -Infinity ? null : Math.floor((from - last) / (7 * DAY));
        return { ...x, weeks, taken: taken.has(x.id) };
      })
      .sort((a, b) => (b.weeks ?? Infinity) - (a.weeks ?? Infinity) || a.name.localeCompare(b.name, 'it', { sensitivity: 'base' }));
  });
  const side = $derived(!!p && p.can_edit && p.people.length > 0);
  function drop(c: Cell, e: DragEvent) {
    e.preventDefault();
    const id = e.dataTransfer?.getData('text/plain') || dragging;
    dragging = null;
    if (!id) return;
    if (accepts(c, id)) choose(c, id);
    else toast(t('drop_not_allowed'), true);
  }

  async function load() {
    try {
      p = await run(api.program(program));
      // Types with a prefill: read the pages once, on first opening.
      if (p.needs_prefill) await prefill(true);
    } catch (e) {
      fail(e);
    }
  }
  load();

  async function prefill(quiet = false) {
    reading = true;
    try {
      p = await run(api.prefillProgram(program));
      if (!quiet) toast(t('prefill_done'));
    } catch (e) {
      if (!quiet) fail(e);
    } finally {
      reading = false;
    }
  }

  function choose(c: Cell, person: string) {
    const k = keyOf(c);
    if (person === (c.assignee ?? '')) delete pending[k];
    else pending[k] = { occurrence: c.occurrence, slot: c.slot, index: c.index, person };
  }
  function write(c: Cell, id: string, value: string) {
    const k = keyOf(c);
    const d = details[k] ?? { occurrence: c.occurrence, slot: c.slot, index: c.index, values: {} };
    d.values[id] = value;
    details[k] = d;
  }
  async function save(quiet = false) {
    if (!changed && !p?.unsent) return true;
    busy = true;
    try {
      // For programs notified at publication, saving also hands the program
      // to who publishes it on the board.
      p = await run(api.assignSlots(program, Object.values(pending), Object.values(details)));
      pending = {};
      details = {};
      if (!quiet) toast(p?.can_send ? t('draft_saved') : t('saved'));
      return true;
    } catch (e) {
      fail(e);
      return false;
    } finally {
      busy = false;
    }
  }
  async function act(kind: 'send' | 'publish') {
    if (!(await save(true))) return;
    try {
      p = kind === 'send' ? await run(api.sendProgram(program)) : await run(api.publishProgram(program));
      toast(t('saved'));
    } catch (e) {
      fail(e);
    }
  }
  async function addExternal() {
    try {
      await run(api.addExternal(external.trim()));
      external = '';
      p = await run(api.program(program));
    } catch (e) {
      fail(e);
    }
  }
</script>

{#snippet fields(c: Cell)}
  {#if c.details.length}
    <div class="details" class:changed={keyOf(c) in details}>
      {#each c.details as f (f.id)}
        {#if p?.can_edit}
          {#if f.kind === 'long_text'}
            <textarea class="wide" rows="2" placeholder={f.label} aria-label={`${c.label} · ${f.label}`} value={detailOf(c, f.id)} oninput={(e) => write(c, f.id, e.currentTarget.value)}></textarea>
          {:else}
            <input
              class={f.kind === 'integer' ? 'num' : 'grow'}
              type={f.kind === 'integer' ? 'number' : 'text'}
              min={f.min ?? undefined}
              placeholder={f.label}
              aria-label={`${c.label} · ${f.label}`}
              value={detailOf(c, f.id)}
              oninput={(e) => write(c, f.id, e.currentTarget.value)}
            />
          {/if}
        {:else if text(f.value)}
          <span class="muted" class:wide={f.kind === 'long_text'}>{f.id === 'minutes' ? `${f.value} min` : f.kind === 'integer' ? `${f.label} ${f.value}` : text(f.value)}</span>
        {/if}
      {/each}
    </div>
  {/if}
{/snippet}

<!-- A slot as a drop target: dashed when empty, with its label; a click opens the list. -->
{#snippet zone(c: Cell)}
  {@const v = valueOf(c)}
  {@const ok = !!dragging && accepts(c, dragging)}
  <div
    class="zone"
    class:filled={!!v}
    class:changed={keyOf(c) in pending}
    class:can={ok}
    class:cannot={!!dragging && !ok}
    role="group"
    aria-label={c.label}
    ondragover={(e) => { if (ok) { e.preventDefault(); if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy'; } }}
    ondrop={(e) => drop(c, e)}
  >
    {#if v}
      <span class="zlabel">{c.label}</span>
      <span class="zname">{nameOf(c, v)}</span>
    {:else}
      <span class="zempty">{c.label}</span>
    {/if}
    {#if p?.can_edit}
      <select class="overlay" aria-label={c.label} value={v} onchange={(e) => choose(c, e.currentTarget.value)}>
        <option value="">—</option>
        {#each c.candidates as o (o.id)}<option value={o.id}>{o.label}</option>{/each}
      </select>
      {#if v}<button class="clear" aria-label={`${t('clear')} · ${c.label}`} onclick={() => choose(c, '')}>×</button>{/if}
    {/if}
    {#if c.assignee && !(keyOf(c) in pending)}<span class="zbadge badge" class:ok={c.response === 'accepted'}>{t(`response_${c.response}` as never)}</span>{/if}
  </div>
{/snippet}

{#snippet rows(occurrence: number, perEvent: boolean)}
  <div class="rows" style={`--cols: ${1 + (p?.columns.length ?? 0)}`}>
    {#each rowsOf(occurrence, perEvent) as c, i (c.slot + c.index)}
      {@const prev = rowsOf(occurrence, perEvent)[i - 1]}
      {#if c.section && c.section !== prev?.section}
        {@const sec = sectionOf(c.section)}
        <div class="band" style={sec?.color ? `background: ${sec.color}` : ''}>{sec?.label}</div>
      {/if}
      <div class="part" class:inline={c.details.every((f) => f.kind === 'integer')}>
        <span class="name">{c.label}</span>
        {@render fields(c)}
      </div>
      <div class="who">{#each inRow(c, null) as x (x.slot)}{#if x.assignable}{@render zone(x)}{/if}{/each}</div>
      {#each p?.columns ?? [] as col (col.id)}
        <div class="who aux">{#each inRow(c, col.id) as x (x.slot)}{@render zone(x)}{/each}</div>
      {/each}
    {/each}
  </div>
{/snippet}

{#if p}
  <div class="stack page wide">
    <div class="spread">
      <h1>{p.title}</h1>
      <span class="badge">{t(`status_${p.status}` as never)}</span>
    </div>
    {#if reading}<p class="muted" role="status">{t('prefill_reading')}</p>{/if}
    {#each p.warnings as w, i (i)}<p class="warn">{w}</p>{/each}
    <!-- Rows scroll away under this band, level with the sticky headers. -->
    <div class="topcover" aria-hidden="true"></div>
    <div class="editor" class:side>
      <div class="groups">
        {#each p.groups as g (g.occurrence)}
          <section class="card stack week">
            <!-- Sticks under the toolbar while the week's rows scroll; leaves with the card. -->
            <div class="whead" style={`--cols: ${1 + p.columns.length}`}>
              <div class="stack tight">
                <h3>{g.label}</h3>
                {#if g.detail}<span class="muted">{g.detail}</span>{/if}
              </div>
              {#if p.columns.length}
                <div class="head muted">{t('main_column')}</div>
                {#each p.columns as col (col.id)}<div class="head muted">{col.label}</div>{/each}
              {/if}
            </div>
            {@render rows(g.occurrence, false)}
            {#each g.days as d (d.occurrence)}
              {#if rowsOf(d.occurrence, true).length}
                <h4 class="day">{d.label}</h4>
                {@render rows(d.occurrence, true)}
              {/if}
            {/each}
          </section>
        {/each}
      </div>
      {#if side}
        <aside class="people card" aria-label={t('people_side')}>
          <h3>{t('people_side')}</h3>
          {#if group}<p class="week-of">{group.label}</p>{/if}
          <p class="muted mini-hint">{t('people_side_hint')}</p>
          <ul>
            {#each people as x (x.id)}
              <li
                class="chip"
                class:taken={x.taken}
                draggable="true"
                ondragstart={(e) => { dragging = x.id; e.dataTransfer?.setData('text/plain', x.id); if (e.dataTransfer) e.dataTransfer.effectAllowed = 'copy'; }}
                ondragend={() => (dragging = null)}
              >
                <span class="cname">{x.name}</span>
                <span class="weeks">{x.taken ? t('weeks_now') : x.weeks === null ? t('weeks_never') : t('weeks_n').replace('{n}', String(x.weeks))}</span>
              </li>
            {/each}
          </ul>
          <form class="external" onsubmit={(e) => { e.preventDefault(); if (external.trim()) addExternal(); }}>
            <input aria-label={t('add_external')} placeholder={t('add_external')} bind:value={external} />
            <button class="btn small" type="submit" disabled={!external.trim()}>{t('add')}</button>
          </form>
        </aside>
      {/if}
    </div>
    <div class="row actions">
      {#if p.can_edit}
        <button class="btn" class:primary={!p.can_send} disabled={busy || (!changed && !p.unsent)} onclick={() => save()}>{t(p.can_send ? 'save' : 'save_plain')}</button>
      {/if}
      {#if p.can_send}<button class="btn primary" disabled={busy} onclick={() => act('send')}>{t('send')}</button>{/if}
      {#if p.can_publish}<button class="btn" disabled={busy} onclick={() => act('publish')}>{t('publish')}</button>{/if}
      {#if p.can_prefill}<button class="btn" disabled={busy || reading} onclick={() => prefill()}>{t('prefill')}</button>{/if}
      {#if changed}<span class="muted" role="status">{t('unsaved_changes').replace('{n}', String(changed))}</span>{/if}
    </div>
    <!-- With the side list it sits at its bottom (here only where the list is hidden). -->
    {#if p.can_edit}
      <div class="row" class:narrow-only={side}>
        <input aria-label={t('add_external')} placeholder={t('add_external')} bind:value={external} style="max-width: 280px" />
        <button class="btn" disabled={!external.trim()} onclick={addExternal}>{t('add')}</button>
      </div>
    {/if}
  </div>
{/if}

<style>
  .page { gap: 16px; }
  .tight { gap: 2px; }
  .groups { display: flex; flex-direction: column; gap: 16px; }
  .details { display: flex; flex-wrap: wrap; gap: 8px; padding: 0 0 8px; }
  .details input, .details textarea { min-height: 34px; font-size: 14px; }
  .details input.grow { flex: 1 1 220px; min-width: 0; }
  .details .wide { flex: 1 1 100%; }
  .details textarea { resize: vertical; font: inherit; font-size: 14px; }
  .editor { width: 100%; }
  .editor.side { display: grid; grid-template-columns: minmax(0, 1fr) 260px; gap: 18px; align-items: start; }
  .people { position: sticky; top: 84px; max-height: calc(100vh - 170px); overflow-y: auto; padding: 14px; display: flex; flex-direction: column; gap: 8px; }
  .people h3 { font-size: 16px; }
  .week-of { margin: -4px 0 0; font-size: 13px; font-weight: 600; color: var(--primary-strong); }
  .mini-hint { font-size: 12px; margin: 0; }
  .people ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .chip { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 8px 10px; border: 1px solid var(--border); border-radius: 10px; background: var(--bg); cursor: grab; font-size: 14px; user-select: none; }
  .chip:active { cursor: grabbing; }
  .chip.taken { opacity: 0.55; }
  .cname { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .weeks { flex: none; font-size: 12px; color: var(--muted); }
  /* Every box the same height, so the rows of boxes line up. */
  .zone { position: relative; height: 64px; flex: none; border: 1.5px dashed var(--line); border-radius: 10px; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; padding: 4px 22px; gap: 1px; }
  .zone.filled { border-style: solid; background: var(--bg); }
  .zone.changed { border-color: var(--primary); box-shadow: 0 0 0 2px var(--primary-soft); }
  .zone.can { border-color: var(--ok, #2e7d32); background: color-mix(in srgb, var(--ok, #2e7d32) 10%, transparent); }
  .zone.cannot { opacity: 0.45; }
  .zempty { font-size: 13px; color: var(--muted); line-height: 1.2; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .zlabel { font-size: 11px; color: var(--muted); line-height: 1.1; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .zname { font-size: 14px; font-weight: 600; line-height: 1.2; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .overlay { position: absolute; inset: 0; opacity: 0; cursor: pointer; width: 100%; height: 100%; min-height: 0; }
  .clear { position: absolute; top: 2px; right: 4px; z-index: 2; border: none; background: none; color: var(--muted); font-size: 16px; cursor: pointer; line-height: 1; padding: 2px 4px; }
  .zbadge { position: absolute; bottom: -8px; right: 6px; font-size: 10px; padding: 0 6px; pointer-events: none; }
  .external { display: flex; flex-direction: column; gap: 6px; margin-top: 6px; padding-top: 10px; border-top: 1px solid var(--border); }
  .external input { min-height: 38px; font-size: 14px; }
  .narrow-only { display: none; }
  @media (max-width: 1000px) { .editor.side { grid-template-columns: minmax(0, 1fr); } .people { display: none; } .narrow-only { display: flex; } }
  /* Parts in rows: 4/5 the part, 1/5 per room. */
  .rows { display: grid; grid-template-columns: minmax(0, 4fr) repeat(var(--cols), minmax(0, 1fr)); }
  .rows > * { padding: 10px 0; border-bottom: 1px solid var(--border); }
  /* Gaps as padding, so the row lines run across every column. */
  .rows > .who { padding-left: 14px; }
  .topcover { position: fixed; top: 0; left: 0; right: 0; height: 84px; background: var(--page); z-index: 4; pointer-events: none; }
  @media (max-width: 760px) { .topcover { display: none; } }
  .whead { position: sticky; top: 84px; z-index: 3; display: grid; grid-template-columns: minmax(0, 4fr) repeat(var(--cols), minmax(0, 1fr)); align-items: end; background: var(--bg); margin: -18px -20px 0; padding: 18px 20px 10px; border-bottom: 1px solid var(--border); border-radius: var(--radius) var(--radius) 0 0; }
  .whead .head { padding-left: 14px; }
  .head { font-size: 13px; font-weight: 600; }
  .band { grid-column: 1 / -1; background: var(--muted); color: #fff; font-weight: 700; text-transform: uppercase; letter-spacing: 0.02em; font-size: 14px; padding: 6px 12px; margin-top: 10px; border-radius: 4px; border-bottom: none; }
  .part { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .part .details { padding: 0; }
  .part.inline { flex-direction: row; align-items: center; gap: 10px; }
  /* As tall as a menu, so the name and the first choice share a line. */
  .name { font-weight: 500; min-height: 64px; display: flex; align-items: center; }
  .part.inline .details input.num { width: 72px; }
  /* Boxes sit at the bottom of their row, level with the part's last field. */
  .who { display: flex; flex-direction: column; justify-content: flex-end; gap: 8px; min-width: 0; }
  @media (max-width: 760px) {
    .rows { grid-template-columns: minmax(0, 1fr); }
    .whead { grid-template-columns: minmax(0, 1fr); top: 0; }
    .whead .head { display: none; }
    .rows > .who { border-bottom: none; padding-top: 0; padding-left: 0; }
    .rows > .who:empty { display: none; }
    .rows > .part { border-bottom: none; }
  }
  .details input.num { width: 84px; flex: none; }
  .details.changed input { border-color: var(--primary); }
  /* Down to the window's edge, so nothing shows below the buttons. */
  .actions { position: sticky; bottom: 0; z-index: 5; background: var(--page); padding: 10px 0 18px; }
  /* Phone: the bar's background runs behind the floating menu. */
  @media (max-width: 760px) { .actions { padding-bottom: calc(100px + var(--inset-bottom, env(safe-area-inset-bottom, 0px))); } }
  .day { margin-top: 10px; font-size: 14px; color: var(--muted); font-weight: 600; }
</style>
