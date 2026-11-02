<script lang="ts">
  import type { Settings } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail, toast } from '../toast.svelte';
  import { isMobile, biometricCheck } from '../mobile';

  let { hasPin, biometric, onchange }: { hasPin: boolean; biometric: boolean; onchange: () => void } = $props();
  let s = $state<Settings | null>(null);
  let pin = $state('');
  run(api.settings()).then((x) => (s = x)).catch(fail);

  async function save() {
    if (!s) return;
    try {
      s = await run(api.updateSettings(s.auto_lock_minutes, s.mobile_data_photos, s.muted_categories));
      toast(t('saved'));
    } catch (e) {
      fail(e);
    }
  }
  async function setPin(value: string | null) {
    try {
      await run(api.setPin(value));
      pin = '';
      toast(t('saved'));
      onchange();
    } catch (e) {
      fail(e);
    }
  }
  async function setBiometric(on: boolean) {
    try {
      if (on) await biometricCheck(t('biometric_reason'));
      await run(api.setBiometric(on));
      toast(t('saved'));
      onchange();
    } catch (e) {
      fail(e);
      onchange();
    }
  }
  function toggle(id: string, on: boolean) {
    if (!s) return;
    s.muted_categories = on ? [...s.muted_categories, id] : s.muted_categories.filter((c) => c !== id);
  }
</script>

{#if s}
  <div class="stack page">
    <h1>{t('settings')}</h1>
    <section class="card stack">
      <label for="lock">{t('auto_lock')}</label>
      <input id="lock" type="number" min="1" max="30" bind:value={s.auto_lock_minutes} style="max-width: 120px" />
      <label class="row"><input type="checkbox" bind:checked={s.mobile_data_photos} style="width: 20px; min-height: 20px" />{t('mobile_photos')}</label>
      {#if s.categories.length}
        <h2>{t('muted')}</h2>
        {#each s.categories as c (c.id)}
          <label class="row"><input type="checkbox" checked={s.muted_categories.includes(c.id)} onchange={(e) => toggle(c.id, e.currentTarget.checked)} style="width: 20px; min-height: 20px" />{c.label}</label>
        {/each}
      {/if}
      <button class="btn primary" onclick={save}>{t('save')}</button>
    </section>
    <section class="card stack">
      <label for="pin">{t('pin')}</label>
      <p class="muted">{t('pin_hint')}</p>
      <input id="pin" type="password" inputmode="numeric" autocomplete="off" bind:value={pin} style="max-width: 200px" />
      <div class="row">
        <button class="btn" disabled={pin.length < 4} onclick={() => setPin(pin)}>{hasPin ? t('change_pin') : t('set_pin')}</button>
        {#if hasPin}<button class="btn" onclick={() => setPin(null)}>{t('remove_pin')}</button>
        <button class="btn" onclick={() => run(api.lock()).then(onchange)}>{t('lock_now')}</button>{/if}
      </div>
      {#if isMobile && hasPin}
        <label class="row"><input type="checkbox" checked={biometric} onchange={(e) => setBiometric(e.currentTarget.checked)} style="width: 20px; min-height: 20px" />{t('biometric_setting')}</label>
      {/if}
    </section>
  </div>
{/if}

<style>
  .page { gap: 16px; }
</style>
