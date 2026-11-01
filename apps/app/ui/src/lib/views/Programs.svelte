<script lang="ts">
  import type { ProgramList } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { go } from '../router.svelte';
  import { fail } from '../toast.svelte';

  let { types }: { types?: string } = $props();
  let list = $state<ProgramList | null>(null);
  let newType = $state('');
  let dates = $state('');

  run(api.programs(types ?? null)).then((l) => { list = l; newType = l.can_draft[0] ?? ''; }).catch(fail);

  async function create() {
    try {
      const ds = dates.split(/\s+/).map((d) => d.trim()).filter(Boolean);
      const id = await run(api.draftProgram(newType, ds));
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
        <label for="dates">{t('dates')}</label>
        <textarea id="dates" bind:value={dates} placeholder="2026-11-05"></textarea>
        <button class="btn primary" disabled={!newType || !dates.trim()} onclick={create}>{t('create')}</button>
      </section>
    {/if}
  </div>
{/if}

<style>
  .page { gap: 14px; }
  .tight { gap: 4px; }
  .link { color: var(--text); text-decoration: none; }
</style>
