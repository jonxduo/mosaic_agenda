<script lang="ts">
  import type { ProgramList, ProgramDates } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { go } from '../router.svelte';
  import Icon from '../components/Icon.svelte';
  import { fail } from '../toast.svelte';

  let { types }: { types?: string } = $props();
  let list = $state<ProgramList | null>(null);
  let newType = $state('');
  let proposal = $state<ProgramDates | null>(null);
  let picked = $state<string[]>([]);
  let creating = $state(false);

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
  <div class="wrap">
  <!-- One tile per program (icon, name, month); the "+" tile opens a new one. -->
  <div class="tiles">
    {#each list.programs as p (p.program_id)}
      <a class="card tile" href="#/program_editor" onclick={(e) => { e.preventDefault(); go('program_editor', { program: p.program_id }); }}>
        <span class="badge state" class:ok={p.status === 'published'}>{label(p.status)}</span>
        <span class="icon"><Icon name={p.icon ?? 'program_list'} size={44} /></span>
        <strong>{p.title}</strong>
        <span class="muted when">{p.subtitle}</span>
      </a>
    {/each}
    {#if list.can_draft.length}
      <button class="card tile add" aria-label={t('new_program')} onclick={() => { creating = true; propose(null); }}>
        <span class="icon"><Icon name="plus" size={44} /></span>
      </button>
    {/if}
  </div>
  {#if creating}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="backdrop" onclick={() => (creating = false)}></div>
    <div class="card stack modal" role="dialog" aria-modal="true" aria-label={t('new_program')} tabindex="-1" onkeydown={(e) => e.key === 'Escape' && (creating = false)}>
      <div class="spread">
        <h3>{t('new_program')}</h3>
        <button class="btn small" onclick={() => (creating = false)}>{t('close')}</button>
      </div>
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
    </div>
  {/if}
  </div>
{/if}

<style>
  /* As many tiles per row as fit. */
  .tiles { display: grid; grid-template-columns: repeat(auto-fill, minmax(170px, 1fr)); gap: 14px; }
  .tile { position: relative; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px; text-align: center; min-height: 170px; color: var(--text); text-decoration: none; cursor: pointer; font: inherit; }
  .tile:hover { border-color: var(--primary); box-shadow: 0 0 0 3px var(--primary-soft); }
  .tile .icon { color: var(--primary); display: grid; place-items: center; width: 72px; height: 72px; border-radius: 18px; background: var(--primary-soft); margin-bottom: 4px; }
  .tile strong { font-size: 16px; }
  .when { font-size: 13px; text-transform: capitalize; }
  .state { position: absolute; top: 10px; right: 10px; font-size: 11px; }
  .add { border-style: dashed; background: none; }
  .add .icon { background: none; color: var(--muted); }
  .backdrop { position: fixed; inset: 0; background: rgb(0 0 0 / 0.35); z-index: 50; }
  .modal { position: fixed; z-index: 51; left: 50%; top: 50%; transform: translate(-50%, -50%); width: min(560px, calc(100vw - 32px)); max-height: calc(100vh - 64px); overflow-y: auto; box-shadow: 0 12px 40px rgb(0 0 0 / 0.2); }
  .period strong { text-align: center; text-transform: capitalize; }
  .dates { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 6px 16px; }
  .date { display: flex; gap: 10px; align-items: center; font-weight: 500; min-height: 40px; }
  .date input { width: 20px; height: 20px; min-height: 20px; margin: 0; accent-color: var(--primary); }
</style>
