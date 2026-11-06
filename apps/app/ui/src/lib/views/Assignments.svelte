<script lang="ts">
  import type { AssignmentItem } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail } from '../toast.svelte';
  import CannotDialog from '../components/CannotDialog.svelte';
  let items = $state<AssignmentItem[]>([]);
  const load = () => run(api.myAssignments()).then((x) => (items = x)).catch(fail);
  load();
  // Accepted unless one says they cannot.
  // "Non posso": the dialog asks for a substitute, or just confirms.
  let cannotItem = $state<AssignmentItem | null>(null);
  const cannot = (a: AssignmentItem) => (cannotItem = a);
</script>

<div class="stack">
  <CannotDialog item={cannotItem} ondone={() => { cannotItem = null; load(); }} />
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
