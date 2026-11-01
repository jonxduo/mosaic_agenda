<script lang="ts">
  import type { AssignmentItem } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail } from '../toast.svelte';
  let items = $state<AssignmentItem[]>([]);
  const load = () => run(api.myAssignments()).then((x) => (items = x)).catch(fail);
  load();
  async function respond(a: AssignmentItem, accept: boolean) {
    await run(api.respond(a.program_id, a.program_type, a.occurrence, a.slot, a.index, accept, null, null)).catch(fail);
    await load();
  }
</script>

<div class="stack">
  {#each items as a (a.program_id + a.slot + a.occurrence + a.index)}
    <div class="card spread">
      <div class="stack tight grow"><h3>{a.title}</h3><span class="muted">{a.subtitle}</span></div>
      {#if a.can_respond}
        <div class="row"><button class="btn" onclick={() => respond(a, false)}>{t('decline')}</button><button class="btn primary" onclick={() => respond(a, true)}>{t('accept')}</button></div>
      {:else}<span class="badge">{t(`response_${a.response}` as never)}</span>{/if}
    </div>
  {/each}
</div>

<style>
  .tight { gap: 4px; }
</style>
