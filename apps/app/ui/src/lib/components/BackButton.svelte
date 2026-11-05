<script lang="ts">
  // The one way back drawn in every page: a purple "‹", then a vertical line
  // that sets it apart from the title next to it.
  import { t } from '../i18n';

  let { onclick }: { onclick: () => void } = $props();
</script>

<button type="button" class="back" aria-label={t('back')} title={t('back')} {onclick}>‹</button>
<span class="sep" aria-hidden="true"></span>

<style>
  .back { flex: none; width: 32px; height: 40px; border: none; background: none; color: var(--primary); font-size: 32px; line-height: 1; cursor: pointer; display: grid; place-items: center; padding: 0 0 4px; border-radius: 8px; }
  .back:hover { color: var(--primary-strong); background: var(--primary-soft); }
  .sep { flex: none; align-self: stretch; width: 1px; min-height: 32px; background: var(--border); }
</style>
