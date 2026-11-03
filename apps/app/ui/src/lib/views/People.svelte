<script lang="ts">
  import { ask } from '../ask.svelte';
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
  let search = $state('');
  let unitFilter = $state('');
  let adding = $state(false);
  let showErased = $state(false);

  const collator = new Intl.Collator('it', { sensitivity: 'base' });
  const fold = (x: string) => x.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();
  const visible = $derived(
    (data?.persons ?? [])
      .filter((p) => showErased || !p.erased)
      .filter((p) => !unitFilter || p.unit === unitFilter)
      .filter((p) => !search.trim() || fold(p.name).includes(fold(search.trim())))
      .sort((a, b) => collator.compare(a.name, b.name))
  );
  const unitNames = $derived([...new Set((data?.persons ?? []).map((p) => p.unit).filter((u): u is string => !!u))].sort(collator.compare));
  const erasedCount = $derived((data?.persons ?? []).filter((p) => p.erased).length);
  const initials = (name: string) =>
    name
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((w) => w[0]?.toUpperCase() ?? '')
      .join('') || '—';
  function toggle(id: string) {
    open = open === id ? null : id;
    invite = null;
    // The form starts from the values the person has.
    editAttrs = { ...(data?.persons.find((p) => p.person_id === id)?.values ?? {}) };
  }

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
    adding = false;
  }
  async function erase(person: string, name: string) {
    if (!(await ask(t('erase_confirm').replace('{name}', name), t('erase_data'), true))) return;
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
    <header class="spread">
      <h1>{t('people')} <span class="count">{visible.length}</span></h1>
      {#if data.can_manage}<button class="btn primary" aria-expanded={adding} onclick={() => (adding = !adding)}>{t('add_person')}</button>{/if}
    </header>

    {#if adding && data.can_manage}
      <section class="card stack">
        <h3>{t('add_person')}</h3>
        <label for="pname">{t('name')}</label>
        <input id="pname" bind:value={name} />
        {#each data.attributes as f (f.id)}<FieldInput field={f} bind:value={attrs[f.id]} />{/each}
        <div class="row">
          <button class="btn primary" disabled={!name.trim()} onclick={addPerson}>{t('add')}</button>
          <button class="btn" onclick={() => (adding = false)}>{t('cancel')}</button>
        </div>
      </section>
    {/if}

    <div class="row toolbar">
      <input class="grow" type="search" placeholder={t('people_search')} aria-label={t('people_search')} bind:value={search} />
      {#if unitNames.length}
        <select aria-label={t('unit')} bind:value={unitFilter}>
          <option value="">{t('people_all_units')}</option>
          {#each unitNames as u (u)}<option value={u}>{u}</option>{/each}
        </select>
      {/if}
      {#if erasedCount}
        <label class="row check"><input type="checkbox" bind:checked={showErased} /> {t('people_show_erased')} ({erasedCount})</label>
      {/if}
    </div>

    {#if !visible.length}<p class="muted">{t('people_none')}</p>{/if}
    <div class="card list">
    {#each visible as p (p.person_id)}
      <section class="person" class:open={open === p.person_id} data-testid="person">
        <button type="button" class="head" aria-expanded={open === p.person_id} onclick={() => toggle(p.person_id)}>
          <span class="avatar" aria-hidden="true">{initials(p.name)}</span>
          <span class="who">
            <span class="name">{p.erased ? t('person_erased') : p.name}{#if me?.person_id === p.person_id}<span class="muted me">({t('you')})</span>{/if}</span>
            {#if p.unit}<span class="unit-inline muted">{p.unit}</span>{/if}
            {#if p.facts.length}
              <span class="facts">
                {#each p.facts as f, i (i)}<span class="fact">{#if f.label}<span class="muted">{f.label}</span> {/if}{f.value}</span>{/each}
              </span>
            {/if}
            {#if p.marks.length || p.roles.length}
              <span class="tags">
                {#each p.marks as m (m)}<span class="badge mark">{m}</span>{/each}
                {#each p.roles as g (g.grant_id)}<span class="badge">{g.label}</span>{/each}
              </span>
            {/if}
          </span>
          <span class="unit muted">{p.unit ?? ''}</span>
          <svg class="chev" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
        </button>
        {#if open === p.person_id && (data.can_manage || data.can_qualify || data.can_grant_roles)}
          <div class="stack edit">
            {#if data.can_manage}
            {#each data.attributes as f (f.id)}<FieldInput field={f} bind:value={editAttrs[f.id]} />{/each}
            <button class="btn small" onclick={() => act(() => run(api.updatePerson(p.person_id, null, editAttrs)))}>{t('save')}</button>
            {/if}
            {#if data.can_qualify}
            <div class="row">
              <select aria-label={t('qualification')} bind:value={q}><option value=""></option>{#each data.qualifications as x (x.id)}<option value={x.id}>{x.label}</option>{/each}</select>
              <input type="date" aria-label={t('from')} bind:value={qFrom} style="max-width: 170px" />
              <button class="btn small" disabled={!q} onclick={() => act(() => run(api.grantQualification(p.person_id, q, qFrom || null, null)))}>{t('grant')}</button>
            </div>
            {/if}
            {#if data.can_manage}
            <div class="row">
              <select aria-label={t('unit')} bind:value={unit}><option value=""></option>{#each data.units as x (x.id)}<option value={x.id}>{x.label}</option>{/each}</select>
              <input type="date" aria-label={t('from')} bind:value={unitFrom} style="max-width: 170px" />
              <button class="btn small" disabled={!unit} onclick={() => act(() => run(api.setMembership(p.person_id, unit, unitFrom || null)))}>{t('set_unit')}</button>
            </div>
            {/if}
            {#if data.can_grant_roles}
            <div class="row">
              <select aria-label={t('role')} bind:value={role}><option value=""></option>{#each data.roles as x (x.id)}<option value={x.id}>{x.label}</option>{/each}</select>
              <select aria-label={t('unit')} bind:value={roleUnit}><option value="">—</option>{#each data.units as x (x.id)}<option value={x.id}>{x.label}</option>{/each}</select>
              <button class="btn small" disabled={!role} onclick={() => act(() => run(api.grantRole(p.person_id, role, roleUnit || null)))}>{t('grant')}</button>
            </div>
            {/if}
            {#if data.can_manage}
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
                <button class="btn small" style="color: var(--danger)" onclick={async () => (await ask(t('revoke_confirm'), t('revoke'), true)) && act(() => run(api.revokeDevice(d.id)))}>{t('revoke')}</button>
              </div>
            {/each}
            <p class="muted">{t('archive_hint')}</p>
            <div class="row" role="group" aria-label={t('privacy_rights')}>
              <button class="btn small" onclick={() => guard(() => exportJson(p.person_id, p.name))}>{t('export_json')}</button>
              <button class="btn small" onclick={() => guard(() => printExport(p.person_id))}>{t('export_print')}</button>
              <button class="btn small" style="color: var(--danger)" onclick={() => erase(p.person_id, p.name)}>{t('erase_data')}</button>
            </div>
            {/if}
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
    </div>
  </div>
{/if}

<style>
  .page { gap: 14px; }
  .count { font-size: 0.6em; font-weight: 600; color: var(--muted); vertical-align: middle; }
  .toolbar { gap: 10px; flex-wrap: wrap; }
  .toolbar select { max-width: 220px; }
  .check { gap: 6px; font-size: 14px; color: var(--muted); flex-wrap: nowrap; }
  .check input { width: 18px; height: 18px; min-height: 0; margin: 0; flex: none; }
  .toolbar input[type='search'] { min-width: 200px; }
  .me { margin-left: 6px; font-weight: 400; }
  .list { padding: 0; overflow: hidden; }
  .person + .person { border-top: 1px solid var(--border); }
  .person.open { background: var(--surface); }
  .head { display: flex; align-items: center; gap: 12px; background: none; border: none; padding: 12px 16px; font: inherit; color: inherit; cursor: pointer; text-align: left; width: 100%; min-height: 56px; }
  .head:hover { background: var(--primary-soft); }
  .avatar { flex: none; width: 36px; height: 36px; border-radius: 50%; display: grid; place-items: center; font-size: 13px; font-weight: 700; background: var(--primary-soft); color: var(--primary-strong); }
  .who { flex: 1 1 auto; min-width: 0; display: flex; flex-direction: column; gap: 4px; }
  .name { font-weight: 600; }
  .tags { display: flex; flex-wrap: wrap; gap: 4px; }
  .facts { display: flex; flex-wrap: wrap; column-gap: 14px; row-gap: 2px; font-size: 14px; }
  .fact { white-space: nowrap; }
  .fact .muted { font-size: 13px; margin-right: 4px; }
  .tags .badge { white-space: nowrap; }
  .head:hover .mark, .open .mark { background: var(--bg); }
  .unit-inline { display: none; font-size: 13px; }
  @media (max-width: 640px) {
    .unit { display: none; }
    .unit-inline { display: block; }
    .facts { column-gap: 10px; }
    .head { align-items: flex-start; }
    .chev { margin-top: 8px; }
  }
  .mark { background: var(--primary-soft); color: var(--primary-strong); border-color: transparent; font-weight: 600; }
  .unit { flex: none; font-size: 14px; }
  .chev { flex: none; color: var(--muted); transition: transform 0.15s; }
  .open .chev { transform: rotate(90deg); }
  .edit { border-top: 1px solid var(--border); padding: 14px 16px 16px; }
  .edit select { max-width: 260px; }
  .qr { width: 240px; background: #fff; padding: 8px; border-radius: 8px; }
  .qr :global(svg) { width: 100%; height: auto; }
</style>
