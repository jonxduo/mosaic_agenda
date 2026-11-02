<script lang="ts">
  import type { ProgramEditor_Serialize as Editor, SlotCell_Serialize as Cell, SlotChange } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail, toast } from '../toast.svelte';

  let { program }: { program: string } = $props();
  let p = $state<Editor | null>(null);
  let external = $state('');
  run(api.program(program)).then((x) => (p = x)).catch(fail);

  // Choices stay here until "Salva bozza" (or "Invia", which saves first).
  let pending = $state<Record<string, SlotChange>>({});
  let busy = $state(false);
  const keyOf = (c: { occurrence: number; slot: string; index: number }) => `${c.occurrence}|${c.slot}|${c.index}`;
  const changed = $derived(Object.keys(pending).length);
  const valueOf = (c: Cell) => pending[keyOf(c)]?.person ?? c.assignee ?? '';

  function choose(c: Cell, person: string) {
    const k = keyOf(c);
    if (person === (c.assignee ?? '')) delete pending[k];
    else pending[k] = { occurrence: c.occurrence, slot: c.slot, index: c.index, person };
  }
  async function save(quiet = false) {
    if (!changed) return true;
    busy = true;
    try {
      p = await run(api.assignSlots(program, Object.values(pending)));
      pending = {};
      if (!quiet) toast(t('draft_saved'));
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

{#snippet cell(c: Cell)}
  <div class="list-row">
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

{#if p}
  <div class="stack page">
    <div class="spread">
      <h1>{p.title}</h1>
      <span class="badge">{t(`status_${p.status}` as never)}</span>
    </div>
    {#each p.warnings as w, i (i)}<p class="warn">{w}</p>{/each}
    {#each p.groups as g (g.occurrence)}
      <section class="card stack">
        <div class="stack tight">
          <h3>{g.label}</h3>
          {#if g.detail}<span class="muted">{g.detail}</span>{/if}
        </div>
        {#each p.cells.filter((c) => c.occurrence === g.occurrence && !c.per_event) as c (c.slot + c.index)}
          {@render cell(c)}
        {/each}
        {#if p.cells.some((c) => c.per_event && g.days.some((d) => d.occurrence === c.occurrence))}
          {#each g.days as d (d.occurrence)}
            <h4 class="day">{d.label}</h4>
            {#each p.cells.filter((c) => c.per_event && c.occurrence === d.occurrence) as c (c.slot + c.index)}
              {@render cell(c)}
            {/each}
          {/each}
        {/if}
      </section>
    {/each}
    <div class="row actions">
      {#if p.can_edit}
        <button class="btn" disabled={busy || !changed} onclick={() => save()}>{t('save')}</button>
      {/if}
      {#if p.can_send}<button class="btn primary" disabled={busy} onclick={() => act('send')}>{t('send')}</button>{/if}
      {#if p.can_publish}<button class="btn" disabled={busy} onclick={() => act('publish')}>{t('publish')}</button>{/if}
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
  select.changed { border-color: var(--primary); box-shadow: 0 0 0 2px var(--primary-soft); }
  .actions { position: sticky; bottom: 12px; background: var(--page); padding: 8px 0; }
  @media (max-width: 640px) { .actions { bottom: 96px; } }
  .day { margin-top: 10px; font-size: 14px; color: var(--muted); font-weight: 600; }
</style>
