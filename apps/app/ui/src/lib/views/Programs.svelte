<script lang="ts">
  import type { ProgramList, ProgramDates } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { go } from '../router.svelte';
  import { fail } from '../toast.svelte';

  let { types }: { types?: string } = $props();
  let list = $state<ProgramList | null>(null);
  let newType = $state('');
  let proposal = $state<ProgramDates | null>(null);
  let picked = $state<string[]>([]);

  run(api.programs(types ?? null)).then((l) => { list = l; newType = l.can_draft[0] ?? ''; }).catch(fail);

  // The dates come from the days of the events (Calendar and rotations).
  async function propose(period: string | null) {
    if (!newType) return;
    try {
      proposal = await run(api.programDates(newType, period));
      picked = [...proposal.dates];
    } catch (e) {
      fail(e);
    }
  }
  $effect(() => {
    if (newType) propose(null);
  });
  const toggle = (d: string) => (picked = picked.includes(d) ? picked.filter((x) => x !== d) : [...picked, d].sort());

  async function create() {
    try {
      const id = await run(api.draftProgram(newType, picked));
      go('program_editor', { program: id });
    } catch (e) {
      fail(e);
    }
  }
  const label = (s: string) => t(`status_${s}` as never);
</script>

{#if list}
  <div class="stack page">
    {#each list.programs as p (p.program_id)}
      <a class="card spread link" href="#/program_editor" onclick={(e) => { e.preventDefault(); go('program_editor', { program: p.program_id }); }}>
        <div class="stack tight grow"><h3>{p.title}</h3><span class="muted">{p.subtitle}</span></div>
        <span class="badge" class:ok={p.status === 'published'}>{label(p.status)}</span>
      </a>
    {/each}
    {#if list.can_draft.length}
      <section class="card stack">
        <h3>{t('new_program')}</h3>
        <select aria-label={t('type')} bind:value={newType}>
          {#each list.types.filter((x) => list?.can_draft.includes(x.id)) as ty (ty.id)}<option value={ty.id}>{ty.label}</option>{/each}
        </select>
        {#if proposal}
          <div class="row period">
            <button class="btn small" aria-label={t('previous')} onclick={() => propose(proposal!.previous)}>‹</button>
            <strong class="grow">{proposal.label}</strong>
            <button class="btn small" aria-label={t('next')} onclick={() => propose(proposal!.next)}>›</button>
          </div>
          {#if proposal.exists}<p class="warn">{t('program_exists')}</p>{/if}
          {#if proposal.dates.length}
            <div class="dates" role="group" aria-label={t('dates')}>
              {#each proposal.dates as d, i (d)}
                <label class="date"><input type="checkbox" checked={picked.includes(d)} onchange={() => toggle(d)} /> {proposal.labels[i]}</label>
              {/each}
            </div>
          {:else}<p class="muted">{t('no_dates')}</p>{/if}
        {/if}
        <button class="btn primary" disabled={!newType || !picked.length} onclick={create}>{t('create')}</button>
      </section>
    {/if}
  </div>
{/if}

<style>
  .page { gap: 14px; }
  .tight { gap: 4px; }
  .link { color: var(--text); text-decoration: none; }
  .period strong { text-align: center; text-transform: capitalize; }
  .dates { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 6px 16px; }
  .date { display: flex; gap: 10px; align-items: center; font-weight: 500; min-height: 40px; }
  .date input { width: 20px; height: 20px; min-height: 20px; margin: 0; accent-color: var(--primary); }
</style>
