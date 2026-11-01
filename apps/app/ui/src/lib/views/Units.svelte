<script lang="ts">
  import type { UnitView, People_Serialize as PeopleData } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail } from '../toast.svelte';
  let units = $state<UnitView[]>([]);
  let types = $state<PeopleData['unit_types']>([]);
  let name = $state(''); let type = $state('');
  async function load() {
    units = await run(api.units()).catch((e) => { fail(e); return []; });
    types = await run(api.people()).then((p) => p.unit_types).catch(() => []);
    type = type || types[0]?.id || '';
  }
  load();
  async function create() {
    try {
      await run(api.createUnit(type, name.trim()));
      name = '';
      await load();
    } catch (e) {
      fail(e);
    }
  }
</script>

<div class="stack page">
  {#each units as u (u.unit_id)}
    <section class="card stack">
      <div class="spread"><h3>{u.name}</h3><span class="muted">{u.unit_type}</span></div>
      {#each u.roles as r (r.label + r.value)}<p><span class="muted">{r.label}:</span> {r.value}</p>{/each}
      <p class="muted">{t('members')}: {u.members.join(', ')}</p>
    </section>
  {/each}
  <section class="card stack">
    <h3>{t('new_unit')}</h3>
    <select aria-label={t('type')} bind:value={type}>{#each types as x (x.id)}<option value={x.id}>{x.label}</option>{/each}</select>
    <input aria-label={t('name')} placeholder={t('name')} bind:value={name} />
    <button class="btn primary" disabled={!name.trim() || !type} onclick={create}>{t('create')}</button>
  </section>
</div>

<style>
  .page { gap: 14px; }
</style>
