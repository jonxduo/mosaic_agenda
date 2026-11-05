<script lang="ts">
  import { api, run } from '../api';
  import { t } from '../i18n';

  // A picture of a photo field: a new one (`data:` URL) or a stored one
  // (reference, opened through the core).
  let { value, alt = '' }: { value: string; alt?: string } = $props();
  let src = $state<string | null>(null);
  let pending = $state(false);
  $effect(() => {
    const v = value;
    src = null;
    pending = false;
    if (v.startsWith('data:')) {
      src = v;
      return;
    }
    run(api.attachment(v)).then((u) => { if (value === v) src = u; }).catch(() => { if (value === v) pending = true; });
  });
</script>

{#if src}
  <img {src} {alt} />
{:else if pending}
  <p class="muted pending">{t('picture_pending')}</p>
{:else}
  <div class="placeholder" aria-busy="true"></div>
{/if}

<style>
  img { display: block; width: 100%; height: auto; border-radius: 10px; border: 1px solid var(--line, #ddd); background: #fff; }
  .placeholder { width: 100%; aspect-ratio: 3 / 1; border-radius: 10px; background: var(--surface-2, #f1f1f1); }
  .pending { padding: 12px; border: 1px dashed var(--line, #ccc); border-radius: 10px; }
</style>
