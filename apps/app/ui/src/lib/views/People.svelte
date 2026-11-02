<script lang="ts">
  import type { People_Serialize as PeopleData, Me, Invitation } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import FieldInput from '../components/FieldInput.svelte';
  import { fail, toast } from '../toast.svelte';
  import { exportJson, printExport } from '../privacy.svelte';

  let { me }: { me: Me | null } = $props();
  let data = $state<PeopleData | null>(null);
  let name = $state('');
  let attrs = $state<Record<string, unknown>>({});
  let open = $state<string | null>(null);
  let q = $state(''); let qFrom = $state('');
  let role = $state(''); let roleUnit = $state('');
  let unit = $state(''); let unitFrom = $state('');
  let invite = $state<Invitation | null>(null);
  let editAttrs = $state<Record<string, unknown>>({});

  async function load() {
    data = await run(api.people()).catch((e) => { fail(e); return null; });
  }
  load();

  async function act(f: () => Promise<unknown>) {
    try {
      await f();
      toast(t('saved'));
      await load();
    } catch (e) {
      fail(e);
    }
  }
  async function addPerson() {
    await act(() => run(api.addPerson(name.trim(), attrs)));
    name = '';
    attrs = {};
  }
  async function erase(person: string, name: string) {
    if (!confirm(t('erase_confirm').replace('{name}', name))) return;
    await act(() => run(api.requestErasure(person)));
  }
  async function guard(f: () => Promise<unknown>) {
    try {
      await f();
    } catch (e) {
      fail(e);
    }
  }
  async function doInvite(person: string) {
    try {
      invite = await run(api.inviteDevice(person));
    } catch (e) {
      fail(e);
    }
  }
</script>

{#if data}
  <div class="stack page">
    <h1>{t('people')}</h1>
    {#each data.persons as p (p.person_id)}
      <section class="card stack" data-testid="person">
        <button type="button" class="head spread" onclick={() => { open = open === p.person_id ? null : p.person_id; invite = null; editAttrs = {}; }}>
          <h3>{p.name}{#if me?.person_id === p.person_id} ·{/if}</h3>
          <span class="muted">{p.unit ?? ''}</span>
        </button>
        <div class="row">
          {#each p.qualifications as g (g.grant_id)}<span class="badge">{g.label}{g.period ? ` (${g.period})` : ''}</span>{/each}
          {#each p.roles as g (g.grant_id)}<span class="badge ok">{g.label}</span>{/each}
        </div>
        {#if open === p.person_id && data.can_manage}
          <div class="stack edit">
            {#each data.attributes as f (f.id)}<FieldInput field={f} bind:value={editAttrs[f.id]} />{/each}
            <button class="btn small" onclick={() => act(() => run(api.updatePerson(p.person_id, null, editAttrs)))}>{t('save')}</button>
            <div class="row">
              <select aria-label={t('qualification')} bind:value={q}><option value=""></option>{#each data.qualifications as x (x.id)}<option value={x.id}>{x.label}</option>{/each}</select>
              <input type="date" aria-label={t('from')} bind:value={qFrom} style="max-width: 170px" />
              <button class="btn small" disabled={!q} onclick={() => act(() => run(api.grantQualification(p.person_id, q, qFrom || null, null)))}>{t('grant')}</button>
            </div>
            <div class="row">
              <select aria-label={t('unit')} bind:value={unit}><option value=""></option>{#each data.units as x (x.id)}<option value={x.id}>{x.label}</option>{/each}</select>
              <input type="date" aria-label={t('from')} bind:value={unitFrom} style="max-width: 170px" />
              <button class="btn small" disabled={!unit} onclick={() => act(() => run(api.setMembership(p.person_id, unit, unitFrom || null)))}>{t('set_unit')}</button>
            </div>
            <div class="row">
              <select aria-label={t('role')} bind:value={role}><option value=""></option>{#each data.roles as x (x.id)}<option value={x.id}>{x.label}</option>{/each}</select>
              <select aria-label={t('unit')} bind:value={roleUnit}><option value="">—</option>{#each data.units as x (x.id)}<option value={x.id}>{x.label}</option>{/each}</select>
              <button class="btn small" disabled={!role} onclick={() => act(() => run(api.grantRole(p.person_id, role, roleUnit || null)))}>{t('grant')}</button>
            </div>
            <div class="row">
              <span class="muted">{t('devices')}: {p.devices.length}</span>
              <button class="btn small" onclick={() => doInvite(p.person_id)}>{t('invite')}</button>
            </div>
            {#each p.devices as d (d.id)}
              <div class="row device">
                <code title={d.id}>{d.id.slice(0, 8)}</code>
                {#if d.label === 'archive'}<span class="badge">{t('archive_device')}</span>{/if}
                <button class="btn small" onclick={() => act(() => run(api.setArchive(d.id, d.label !== 'archive')))}>
                  {d.label === 'archive' ? t('archive_unset') : t('archive_set')}
                </button>
                <button class="btn small" style="color: var(--danger)" onclick={() => confirm(t('revoke_confirm')) && act(() => run(api.revokeDevice(d.id)))}>{t('revoke')}</button>
              </div>
            {/each}
            <p class="muted">{t('archive_hint')}</p>
            <div class="row" role="group" aria-label={t('privacy_rights')}>
              <button class="btn small" onclick={() => guard(() => exportJson(p.person_id, p.name))}>{t('export_json')}</button>
              <button class="btn small" onclick={() => guard(() => printExport(p.person_id))}>{t('export_print')}</button>
              <button class="btn small" style="color: var(--danger)" onclick={() => erase(p.person_id, p.name)}>{t('erase_data')}</button>
            </div>
            {#if invite}
              <div class="stack invite">
                <p class="muted">{t('invite_hint')} {invite.expires_at}</p>
                <div class="qr">{@html invite.qr_svg}</div>
                <textarea readonly value={invite.ticket} aria-label={t('ticket')}></textarea>
                <button class="btn small" onclick={() => navigator.clipboard?.writeText(invite!.ticket).then(() => toast(t('copied')))}>{t('copy')}</button>
              </div>
            {/if}
          </div>
        {/if}
      </section>
    {/each}

    {#if data.can_manage}
      <section class="card stack">
        <h3>{t('add_person')}</h3>
        <label for="pname">{t('name')}</label>
        <input id="pname" bind:value={name} />
        {#each data.attributes as f (f.id)}<FieldInput field={f} bind:value={attrs[f.id]} />{/each}
        <button class="btn primary" disabled={!name.trim()} onclick={addPerson}>{t('add')}</button>
      </section>
    {/if}
  </div>
{/if}

<style>
  .page { gap: 14px; }
  .head { background: none; border: none; padding: 0; font: inherit; color: inherit; cursor: pointer; text-align: left; width: 100%; }
  .edit { border-top: 1px solid var(--border); padding-top: 14px; }
  .edit select { max-width: 260px; }
  .qr { width: 240px; background: #fff; padding: 8px; border-radius: 8px; }
  .qr :global(svg) { width: 100%; height: auto; }
</style>
