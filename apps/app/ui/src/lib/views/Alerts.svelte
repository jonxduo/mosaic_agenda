<script lang="ts">
  import type { Alert } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail } from '../toast.svelte';
  let items = $state<Alert[]>([]);
  run(api.alerts()).then((x) => (items = x)).catch(fail);
</script>

<div class="stack">
  <h1>{t('alerts')}</h1>
  {#if !items.length}<p class="muted">{t('no_alerts')}</p>{/if}
  {#each items as a, i (i)}<div class="list-row"><span class="badge">{a.kind}</span><span class="grow">{a.message}</span></div>{/each}
</div>
