<script lang="ts">
  import type { ProgramEditor_Serialize as Editor, SlotCell_Serialize as Cell, SlotChange, DetailChange } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail, toast } from '../toast.svelte';

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
  // Lists of duties (no details to write) sit two per row on wide screens.
  const compact = $derived(!!p && !p.cells.some((c) => c.details.length));

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

{#snippet choice(c: Cell, pair = false)}
  <div class="list-row" class:pair>
    <span class="grow">{c.label}</span>
    {#if p?.can_edit}
      <select aria-label={c.label} class:changed={keyOf(c) in pending} value={valueOf(c)} onchange={(e) => choose(c, e.currentTarget.value)} style="max-width: 260px">
        <option value="">{t('unassigned')}</option>
        {#each c.candidates as o (o.id)}<option value={o.id}>{o.label}</option>{/each}
      </select>
    {:else}<span>{c.assignee_name ?? '—'}</span>{/if}
    {#if c.assignee && !(keyOf(c) in pending)}<span class="badge" class:ok={c.response === 'accepted'}>{t(`response_${c.response}` as never)}</span>{/if}
  </div>
{/snippet}

{#snippet cell(c: Cell)}
  <div class="slot">
    {@render choice(c)}
    <!-- Who is chosen together with this slot (e.g. a householder) sits right under it. -->
    {#if c.partner}
      {@const mate = p?.cells.find((x) => x.slot === c.partner && x.occurrence === c.occurrence && x.index === c.index)}
      {#if mate}{@render choice(mate, true)}{/if}
    {/if}
    {#if c.details.length}
      <div class="details" class:changed={keyOf(c) in details}>
        {#each c.details as f (f.id)}
          {#if p?.can_edit}
            {#if f.kind === 'long_text'}
              <textarea
                class="wide"
                rows="2"
                placeholder={f.label}
                aria-label={`${c.label} · ${f.label}`}
                value={detailOf(c, f.id)}
                oninput={(e) => write(c, f.id, e.currentTarget.value)}
              ></textarea>
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
            <span class="muted" class:wide={f.kind === 'long_text'}>{f.kind === 'integer' ? `${f.value} min` : text(f.value)}</span>
          {/if}
        {/each}
      </div>
    {/if}
  </div>
{/snippet}

{#if p}
  <div class="stack page">
    <div class="spread">
      <h1>{p.title}</h1>
      <span class="badge">{t(`status_${p.status}` as never)}</span>
    </div>
    {#if reading}<p class="muted" role="status">{t('prefill_reading')}</p>{/if}
    {#each p.warnings as w, i (i)}<p class="warn">{w}</p>{/each}
    <div class="groups" class:compact>
      {#each p.groups as g (g.occurrence)}
        <section class="card stack">
          <div class="stack tight">
            <h3>{g.label}</h3>
            {#if g.detail}<span class="muted">{g.detail}</span>{/if}
          </div>
          {#each p.cells.filter((c) => c.occurrence === g.occurrence && !c.per_event && !c.paired) as c (c.slot + c.index)}
            {@render cell(c)}
          {/each}
          {#if p.cells.some((c) => c.per_event && g.days.some((d) => d.occurrence === c.occurrence))}
            {#each g.days as d (d.occurrence)}
              <h4 class="day">{d.label}</h4>
              {#each p.cells.filter((c) => c.per_event && c.occurrence === d.occurrence && !c.paired) as c (c.slot + c.index)}
                {@render cell(c)}
              {/each}
            {/each}
          {/if}
        </section>
      {/each}
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
    {#if p.can_edit}
      <div class="row">
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
  @media (min-width: 900px) {
    .groups.compact { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); align-items: start; }
  }
  select.changed { border-color: var(--primary); box-shadow: 0 0 0 2px var(--primary-soft); }
  .details { display: flex; flex-wrap: wrap; gap: 8px; padding: 0 0 8px; }
  .details input, .details textarea { min-height: 34px; font-size: 14px; }
  .details input.grow { flex: 1 1 220px; min-width: 0; }
  .details .wide { flex: 1 1 100%; }
  .details textarea { resize: vertical; font: inherit; font-size: 14px; }
  .pair { padding-left: 18px; border-left: 2px solid var(--primary-soft); }
  .details input.num { width: 84px; flex: none; }
  .details.changed input { border-color: var(--primary); }
  .actions { position: sticky; bottom: 12px; background: var(--page); padding: 8px 0; }
  @media (max-width: 640px) { .actions { bottom: 96px; } }
  .day { margin-top: 10px; font-size: 14px; color: var(--muted); font-weight: 600; }
</style>
