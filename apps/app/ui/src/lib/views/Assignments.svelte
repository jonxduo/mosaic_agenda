<script lang="ts">
  import type { AssignmentItem } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail } from '../toast.svelte';
  import { ask } from '../ask.svelte';
  let items = $state<AssignmentItem[]>([]);
  const load = () => run(api.myAssignments()).then((x) => (items = x)).catch(fail);
  load();
  // Accepted unless one says they cannot.
  async function cannot(a: AssignmentItem) {
    if (!(await ask(t('cannot_confirm').replace('{what}', `${a.title} · ${a.subtitle}`), t('decline'), true))) return;
    await run(api.respond(a.program_id, a.program_type, a.occurrence, a.slot, a.index, false, null, null)).catch(fail);
    await load();
  }
</script>

<div class="stack">
  {#each items as a (a.program_id + a.slot + a.occurrence + a.index)}
    <div class="card spread">
      <div class="stack tight grow"><h3>{a.title}</h3><span class="muted">{a.subtitle}</span></div>
      {#if a.response === 'declined'}<span class="badge warn">{t('response_declined')}</span>
      {:else if a.can_respond}<button class="btn small" onclick={() => cannot(a)}>{t('decline')}</button>{/if}
    </div>
  {/each}
</div>

<style>
  .tight { gap: 4px; }
</style>
