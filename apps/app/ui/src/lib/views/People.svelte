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
  // Roles: ticked or not; a role that supervises picks its unit.
  let roles = $state<Record<string, boolean>>({});
  let supervised = $state<Record<string, string>>({});
  // The edit form: dates of the dated qualifications (e.g. baptism) and a box
  // with a date for each appointment; empty texts mean "not held".
  let dates = $state<Record<string, string>>({});
  let appts = $state<Record<string, { on: boolean; from: string }>>({});
  let saving = $state(false);
  const today = () => new Date().toLocaleDateString('sv');
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
    const person = data?.persons.find((p) => p.person_id === id);
    editAttrs = { ...(person?.values ?? {}) };
    const held = (q: string) => person?.held.find((h) => h.qualification === q);
    dates = Object.fromEntries((data?.dated ?? []).map((q) => [q.id, held(q.id)?.from ?? '']));
    appts = Object.fromEntries((data?.appointments ?? []).map((q) => [q.id, { on: !!held(q.id), from: held(q.id)?.from ?? '' }]));
    unit = ''; unitFrom = '';
    roles = Object.fromEntries((data?.role_options ?? []).map((r) => [r.id, !!person?.held_roles.some((h) => h.role === r.id)]));
    supervised = Object.fromEntries((data?.role_options ?? []).map((r) => [r.id, person?.held_roles.find((h) => h.role === r.id)?.unit ?? '']));
  }
  function check(q: string, on: boolean) {
    // A box ticked without a date starts today.
    appts[q] = { on, from: on ? appts[q].from || today() : appts[q].from };
  }

  /** One save for the data, the qualifications, the group and a new role:
   * ends first (so an appointment can replace one it excludes), then grants. */
  async function save(id: string) {
    const person = data?.persons.find((p) => p.person_id === id);
    if (!person || !data) return;
    const held = (q: string) => person.held.find((h) => h.qualification === q);
    const ends: Array<() => Promise<unknown>> = [];
    const grants: Array<() => Promise<unknown>> = [];
    // A changed or cleared date is a correction: the old grant never held.
    const replace = (q: string, from: string) => {
      const h = held(q);
      if (h && h.from !== from) ends.push(() => run(api.endGrant('qualification', h.grant_id, h.from)));
      if (from && (!h || h.from !== from)) grants.push(() => run(api.grantQualification(id, q, from, null)));
    };
    if (data.can_qualify) {
      for (const q of data.dated) replace(q.id, dates[q.id] ?? '');
      for (const q of data.appointments) {
        const a = appts[q.id];
        const h = held(q.id);
        if (!a?.on && h) ends.push(() => run(api.endGrant('qualification', h.grant_id, null)));
        else if (a?.on) replace(q.id, a.from || today());
      }
    }
    // Roles: a unit role is held in the person's unit of that type (the one
    // chosen here if it changes), a supervising role in the unit picked.
    const roleEnds: Array<() => Promise<unknown>> = [];
    const roleGrants: Array<() => Promise<unknown>> = [];
    if (data.can_grant_roles) {
      for (const r of data.role_options) {
        const held = person.held_roles.filter((h) => h.role === r.id);
        let where: string | null = null;
        if (roles[r.id] && r.unit_type) {
          where = r.choose_unit
            ? supervised[r.id] || null
            : (unit && data.units.some((u) => u.id === unit) ? unit : null) ?? person.unit_ids.find((u) => u.unit_type === r.unit_type)?.unit_id ?? null;
          if (!where) return toast(t(r.choose_unit ? 'role_needs_supervised' : 'role_needs_unit').replace('{name}', r.label), true);
        }
        const keep = roles[r.id] && held.some((h) => (h.unit ?? null) === where);
        for (const h of held) if (!keep || (h.unit ?? null) !== where) roleEnds.push(() => run(api.endGrant('role', h.grant_id, null)));
        if (roles[r.id] && !keep) roleGrants.push(() => run(api.grantRole(id, r.id, where)));
      }
    }
    saving = true;
    try {
      if (data.can_manage) await run(api.updatePerson(id, null, editAttrs));
      for (const f of [...ends, ...grants]) await f();
      if (data.can_manage && unit) await run(api.setMembership(id, unit, unitFrom || null));
      for (const f of [...roleEnds, ...roleGrants]) await f();
      toast(t('saved'));
    } catch (e) {
      fail(e);
    } finally {
      saving = false;
      await load();
      toggleKeep(id);
    }
  }
  function toggleKeep(id: string) {
    open = null;
    toggle(id);
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
              <div class="row privacy" role="group" aria-label={t('privacy_rights')}>
                <button class="btn small" onclick={() => guard(() => exportJson(p.person_id, p.name))}>{t('export_json')}</button>
                <button class="btn small" onclick={() => guard(() => printExport(p.person_id))}>{t('export_print')}</button>
                <button class="btn small" style="color: var(--danger)" onclick={() => erase(p.person_id, p.name)}>{t('erase_data')}</button>
              </div>
            {/if}
            {#if data.can_manage || data.dated.length}
              <h4 class="sub">{t('person_data')}</h4>
              <div class="grid">
                {#if data.can_manage}
                  {#each data.attributes.filter((f) => !data?.with_devices.includes(f.id)) as f (f.id)}<FieldInput field={f} bind:value={editAttrs[f.id]} />{/each}
                {/if}
                {#if data.can_qualify}
                  {#each data.dated as q (q.id)}
                    <div class="stack small">
                      <label for={`d-${q.id}`}>{t('date_of').replace('{what}', q.label)}</label>
                      <input id={`d-${q.id}`} type="date" bind:value={dates[q.id]} />
                    </div>
                  {/each}
                {/if}
              </div>
            {/if}
            {#if data.can_qualify && data.appointments.length}
              <h4 class="sub">{t('appointments')}</h4>
              <div class="appts">
                {#each data.appointments as q (q.id)}
                  <div class="appt" class:on={appts[q.id]?.on}>
                    <label class="row tick"><input type="checkbox" checked={appts[q.id]?.on} onchange={(e) => check(q.id, e.currentTarget.checked)} /> {q.label}</label>
                    <input type="date" aria-label={`${q.label} · ${t('from')}`} disabled={!appts[q.id]?.on} bind:value={appts[q.id].from} />
                  </div>
                {/each}
              </div>
            {/if}
            {#if data.can_manage || data.can_grant_roles}
              <h4 class="sub">{t('group_and_roles')}</h4>
              <div class="grid">
                {#if data.can_manage}
                  <div class="stack small">
                    <label for="pu">{t('unit')}</label>
                    <select id="pu" bind:value={unit}><option value="">{p.unit ?? '—'}</option>{#each data.units as x (x.id)}<option value={x.id}>{x.label}</option>{/each}</select>
                  </div>
                  <div class="stack small">
                    <label for="puf">{t('from')}</label>
                    <input id="puf" type="date" bind:value={unitFrom} disabled={!unit} />
                  </div>
                {/if}
              </div>
              {#if data.can_grant_roles}
                <div class="appts">
                  {#each data.role_options as r (r.id)}
                    <label class="appt row tick" class:on={roles[r.id]}><input type="checkbox" bind:checked={roles[r.id]} /> {r.label}</label>
                  {/each}
                </div>
                {#each data.role_options.filter((r) => r.choose_unit && roles[r.id]) as r (r.id)}
                  <div class="stack small supervised">
                    <label for={`sup-${r.id}`}>{t('supervised_unit')}{data.role_options.filter((x) => x.choose_unit).length > 1 ? ` · ${r.label}` : ''}</label>
                    <select id={`sup-${r.id}`} bind:value={supervised[r.id]}>
                      <option value=""></option>
                      {#each r.units as u (u.id)}<option value={u.id}>{u.label}</option>{/each}
                    </select>
                  </div>
                {/each}
              {/if}
            {/if}
            <button class="btn primary" disabled={saving} onclick={() => save(p.person_id)}>{t('save_plain')}</button>
            {#if data.can_manage}
              <div class="below stack">
                {#each data.attributes.filter((f) => data?.with_devices.includes(f.id)) as f (f.id)}
                  <label class="row tick">
                    <input type="checkbox" checked={p.values[f.id] === true} onchange={(e) => act(() => run(api.updatePerson(p.person_id, null, { [f.id]: e.currentTarget.checked })))} />
                    {f.label}
                  </label>
                {/each}
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
  .edit { border-top: 1px solid var(--border); padding: 14px 16px 16px; gap: 14px; }
  .privacy { justify-content: flex-end; flex-wrap: wrap; gap: 6px; }
  .sub { font-size: 13px; text-transform: uppercase; letter-spacing: 0.04em; color: var(--muted); margin: 4px 0 -4px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 12px 16px; align-items: end; }
  .appts { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 8px 16px; }
  .appt { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 6px 10px; border: 1px solid var(--border); border-radius: 10px; }
  .appt.on { border-color: var(--primary); background: var(--primary-soft); }
  label.appt { justify-content: flex-start; min-height: 46px; }
  .supervised { max-width: 360px; }
  .appt input[type='date'] { width: 160px; min-height: 36px; font-size: 14px; padding: 4px 8px; }
  .tick { gap: 8px; flex-wrap: nowrap; font-weight: 500; cursor: pointer; }
  .tick input { width: 18px; height: 18px; min-height: 0; margin: 0; flex: none; accent-color: var(--primary); }
  .below { border-top: 1px solid var(--border); padding-top: 12px; gap: 10px; }
  .edit select { max-width: 260px; }
  .qr { width: 240px; background: #fff; padding: 8px; border-radius: 8px; }
  .qr :global(svg) { width: 100%; height: auto; }
</style>
