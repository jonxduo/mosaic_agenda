<script lang="ts">
  // Programs handed in together for publication (a publish group) for one
  // period: who publishes sees them as one program and publishes them at
  // once; a missing one shows the program as incomplete.
  import type { ProgramGroupView as Group } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { go } from '../router.svelte';
  import { fail, toast } from '../toast.svelte';
  import PageHeading from '../components/PageHeading.svelte';
  import Program from './Program.svelte';

  let { group, period, onchange }: { group: string; period: string; onchange: () => void } = $props();
  let g = $state<Group | null>(null);
  let busy = $state(false);
  // Remounts the programs after publishing, so they show the new state.
  let round = $state(0);

  async function load() {
    try {
      g = await run(api.programGroup(group, period));
    } catch (e) {
      fail(e);
    }
  }
  load();

  async function publish() {
    busy = true;
    try {
      g = await run(api.publishProgramGroup(group, period));
      round++;
      toast(t('saved'));
      onchange();
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if g}
  <div class="stack page wide">
    <div class="spread head">
      <PageHeading title={g.title} subtitle={g.period_label} onback={() => go('program_list')} />
      <span class="badge" class:ok={g.status === 'published'}>{t(`status_${g.status}` as never)}</span>
    </div>
    {#if g.missing.length}
      <p class="incomplete" role="status">{t('group_missing').replace('{list}', g.missing.join(', '))}</p>
    {/if}
    {#key round}
      {#each g.parts as p (p.program_id)}
        <Program program={p.program_id} readonly titled sent />
      {/each}
    {/key}
    {#if g.can_publish}
      <div class="row actions">
        <button class="btn primary" disabled={busy} onclick={publish}>{t('publish')}</button>
        {#if g.missing.length}<span class="muted">{t('group_publish_partial')}</span>{/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .page { gap: 16px; }
  .head { align-items: center; }
  .incomplete { margin: 0; padding: 10px 14px; border-radius: 10px; background: #fff4e0; color: #7a4b00; font-size: 14px; }
  .actions { position: sticky; bottom: 0; background: var(--page); padding: 10px 0 18px; z-index: 5; }
  @media (max-width: 760px) { .actions { padding-bottom: calc(100px + var(--inset-bottom, env(safe-area-inset-bottom, 0px))); } }
</style>
