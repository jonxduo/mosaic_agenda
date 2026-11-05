<script lang="ts">
  // The one way back drawn in every page: a square button with "‹".
  import { t } from '../i18n';

  let { onclick }: { onclick: () => void } = $props();
</script>

<button type="button" class="back" aria-label={t('back')} title={t('back')} {onclick}>‹</button>

<style>
  .back { flex: none; width: 40px; height: 40px; border-radius: 12px; border: 1px solid var(--border); background: none; color: var(--primary); font-size: 24px; line-height: 1; cursor: pointer; display: grid; place-items: center; padding: 0 0 3px; }
  .back:hover { border-color: var(--primary); }
</style>
