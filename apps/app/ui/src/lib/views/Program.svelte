<script lang="ts">
  import type { ProgramEditor_Serialize as Editor, SlotCell_Serialize as Cell } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail, toast } from '../toast.svelte';

  let { program }: { program: string } = $props();
  let p = $state<Editor | null>(null);
  let external = $state('');
  run(api.program(program)).then((x) => (p = x)).catch(fail);

  async function assign(occurrence: number, slot: string, index: number, person: string) {
    try {
      p = await run(api.assignSlot(program, occurrence, slot, index, person));
    } catch (e) {
      fail(e);
    }
  }
  async function act(kind: 'send' | 'publish') {
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
      <select aria-label={c.label} value={c.assignee ?? ''} onchange={(e) => assign(c.occurrence, c.slot, c.index, e.currentTarget.value)} style="max-width: 260px">
        <option value="">{t('unassigned')}</option>
        {#each c.candidates as o (o.id)}<option value={o.id}>{o.label}</option>{/each}
      </select>
    {:else}<span>{c.assignee_name ?? '—'}</span>{/if}
    {#if c.assignee}<span class="badge" class:ok={c.response === 'accepted'}>{t(`response_${c.response}` as never)}</span>{/if}
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
    <div class="row">
      {#if p.can_send}<button class="btn primary" onclick={() => act('send')}>{t('send')}</button>{/if}
      {#if p.can_publish}<button class="btn" onclick={() => act('publish')}>{t('publish')}</button>{/if}
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
  .day { margin-top: 10px; font-size: 14px; color: var(--muted); font-weight: 600; }
</style>
