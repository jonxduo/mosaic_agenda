<script lang="ts">
  import { ask } from '../ask.svelte';
  // Calendar and rotations: the days of the recurring events and, for the
  // programs that cover several events, how long the same persons keep a
  // slot and which slots are assigned. It edits calendar.yaml and
  // programs.yaml like the guided editors and publishes a new version.
  import type { ConfigFile, ConfigIssue, ConfigVersion_Serialize as ConfigVersion, SeriesList } from '../bindings';
  import { api, run } from '../api';
  import { t, type Key } from '../i18n';
  import { fail, toast } from '../toast.svelte';
  import Icon from '../components/Icon.svelte';
  import { save as saveDialog } from '@tauri-apps/plugin-dialog';
  import { inBrowser, download } from '../backup';
  import { parseDoc, toText, toJs, setAt, addTo, addRetire, setText, flatTexts, type Js } from '../yamlEdit';

  let { onchange }: { onchange: () => void } = $props();

  const DAYS = ['MO', 'TU', 'WE', 'TH', 'FR', 'SA', 'SU'];
  type Ev = { i: number; id: string; label: string; days: string[]; time: string };
  // j: position in programs.yaml (null for a slot added here).
  const SLOT_MODES = ['program', 'each_event', 'month', 'dedicated'];
  type Slot = { j: number | null; id: string; key: string; label: string; enabled: boolean; count: number; repeated: boolean; mode: string; removed: boolean };
  // units: the unit type its slots go to (groups), or '' for persons.
  type Prog = { i: number; id: string; label: string; rotation: string; period: string; slots: Slot[]; requires: unknown; adding: string; units: string };
  // An option of a program type (e.g. a second room): i program, k option.
  // events: the event types of its program (the option sits in that box).
  type Opt = { i: number; k: number; id: string; program: string; label: string; enabled: boolean; events: string[] };
  // Weekly series (data, saved one by one): day 1-7, time, place, units, who conducts.
  type SeriesRow = { id: string | null; event_type: string; weekday: number; time: string; location: string; units: string[]; conducted_by_unit: boolean; saved: string };

  let current = $state<ConfigVersion | null>(null);
  let events = $state<Ev[]>([]);
  let programs = $state<Prog[]>([]);
  let options = $state<Opt[]>([]);
  let seriesList = $state<SeriesList | null>(null);
  let seriesRows = $state<SeriesRow[]>([]);
  const rowKey = (m: SeriesRow) => JSON.stringify([m.weekday, m.time, m.location, [...m.units].sort(), m.conducted_by_unit]);
  async function loadSeries() {
    try {
      seriesList = await run(api.calendarSeries());
      seriesRows = seriesList.items.map((x) => {
        const m: SeriesRow = { id: x.id, event_type: x.event_type, weekday: x.weekday, time: x.time, location: x.location, units: [...x.units], conducted_by_unit: x.conducted_by_unit, saved: '' };
        m.saved = rowKey(m);
        return m;
      });
    } catch {
      seriesList = null;
    }
  }
  loadSeries();
  // The day chosen in each box (1-7): at first the first one with a series.
  let selDay = $state<Record<string, number>>({});
  const dayOf = (et: string) => {
    const days = seriesRows.filter((m) => m.event_type === et).map((m) => m.weekday);
    return selDay[et] ?? (days.length ? Math.min(...days) : 1);
  };
  function addSeries(event_type: string, weekday: number) {
    selDay[event_type] = weekday;
    seriesRows.push({ id: null, event_type, weekday, time: '09:30', location: '', units: seriesList?.units.map((u) => u.id) ?? [], conducted_by_unit: false, saved: '' });
  }
  async function saveSeries(m: SeriesRow) {
    try {
      m.id = await run(api.saveSeries(m.id, m.event_type, m.weekday, m.time, m.location, m.units, m.conducted_by_unit));
      m.saved = rowKey(m);
      toast(t('saved'));
    } catch (e) {
      fail(e);
    }
  }
  async function removeSeries(m: SeriesRow) {
    if (m.id && !(await ask(t('sch_series_remove'), t('g_remove'), true))) return;
    try {
      if (m.id) await run(api.removeSeriesItem(m.id));
      seriesRows.splice(seriesRows.indexOf(m), 1);
    } catch (e) {
      fail(e);
    }
  }
  // The whole calendar and rotations as a file (JSON or YAML), and back.
  let fileFormat = $state<'json' | 'yaml'>('json');
  async function exportFile() {
    const name = `calendario-${new Date().toISOString().slice(0, 10)}.${fileFormat}`;
    try {
      if (!inBrowser()) {
        const path = await saveDialog({ defaultPath: name, filters: [{ name: fileFormat.toUpperCase(), extensions: [fileFormat] }] });
        if (!path) return;
        await run(api.scheduleExportTo(fileFormat, path));
      } else {
        const text = await run(api.scheduleExport(fileFormat));
        download(new TextEncoder().encode(text), name, fileFormat === 'json' ? 'application/json' : 'application/yaml');
      }
      toast(t('sch_exported'));
    } catch (e) {
      fail(e);
    }
  }
  async function importFile(input: HTMLInputElement) {
    const f = input.files?.[0];
    input.value = '';
    if (!f || !(await ask(t('sch_import_confirm'), t('sch_import'), true))) return;
    busy = true;
    try {
      const n = await run(api.scheduleImport(await f.text()));
      toast(t('sch_saved').replace('{n}', String(n)));
      await Promise.all([load(), loadSeries()]);
      onchange();
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }
  const toggleUnit = (m: SeriesRow, id: string) => (m.units = m.units.includes(id) ? m.units.filter((x) => x !== id) : [...m.units, id]);
  const unified = (m: SeriesRow) => !!seriesList && seriesList.units.length > 0 && seriesList.units.every((u) => m.units.includes(u.id));
  let busy = $state(false);
  let issues = $state<ConfigIssue[]>([]);
  let initial = '';
  let locale = 'it';
  let texts: Record<string, string> = {};
  // The unit type slots can go to instead of persons (the package's first).
  let unitType = $state('');

  const list = (v: unknown): Js[] => (Array.isArray(v) ? (v as Js[]) : []);
  const file = (path: string) => current?.files.find((f) => f.path === path);
  const snapshot = () => JSON.stringify({ events, programs, options });
  const dirty = $derived(current !== null && snapshot() !== initial);

  async function load() {
    try {
      current = await run(api.configFiles());
      const pkg = toJs(file('package.yaml')?.text);
      locale = String(pkg?.default_locale ?? 'it');
      texts = flatTexts(toJs(file(`texts/${locale}.yaml`)?.text));
      unitType = String(list(toJs(file('units.yaml')?.text)?.unit_types)[0]?.id ?? '');
      const label = (k: unknown) => texts[String(k)] ?? String(k);
      const cal = toJs(file('calendar.yaml')?.text);
      events = list(cal?.event_types)
        .map((e, i) => {
          const rule = String(e.default_recurrence ?? '');
          const by = /FREQ=WEEKLY/.test(rule) ? (rule.match(/BYDAY=([A-Z,]+)/)?.[1] ?? '') : null;
          return { i, id: String(e.id), label: label(e.label), days: by === null ? null : by.split(',').filter(Boolean), time: String(e.default_time ?? '') };
        })
        .filter((e): e is Ev => e.days !== null);
      const prog = toJs(file('programs.yaml')?.text);
      programs = list(prog?.program_types)
        // Programs covering several kinds of events: the others have one per period.
        .map((p, i) => ({ p, i }))
        .filter(({ p }) => Array.isArray(p.event_type) && (p.event_type as unknown[]).length > 1)
        .map(({ p, i }) => ({
          i,
          id: String(p.id),
          label: label(p.label),
          rotation: String(p.rotation ?? 'occurrence'),
          period: String(p.period ?? 'week'),
          units: p.assign_units ? String(p.assign_units) : '',
          // New slots take the program's requirement (`slot_requires`),
          // whatever their name.
          requires: p.slot_requires ?? list(p.slots)[0]?.requires,
          adding: '',
          slots: list(p.slots).map((s, j) => ({
            j,
            id: String(s.id),
            key: String(s.label),
            label: label(s.label),
            enabled: s.enabled !== false,
            count: Number(s.count ?? 1),
            repeated: s.repeat !== undefined,
            // How the persons change: program (default), each_event, month, dedicated.
            mode: String(s.mode ?? (s.each_event === true ? 'each_event' : 'program')),
            removed: false
          }))
        }));
      options = list(prog?.program_types).flatMap((p, i) =>
        list(p.options).map((o, k) => ({
          i, k, id: `${String(p.id)}.${String(o.id)}`, program: label(p.label), label: label(o.label), enabled: o.enabled === true,
          events: (Array.isArray(p.event_type) ? (p.event_type as unknown[]) : [p.event_type]).map(String)
        }))
      );
      initial = snapshot();
    } catch (e) {
      fail(e);
    }
  }
  load();

  const toggleDay = (e: Ev, d: string) => (e.days = e.days.includes(d) ? e.days.filter((x) => x !== d) : DAYS.filter((x) => x === d || e.days.includes(x)));

  /** A new slot id from its name: lowercase ascii, unique in the package. */
  function slugFor(name: string): string {
    const used = new Set([
      ...list(toJs(file('programs.yaml')?.text)?.program_types).flatMap((p) => list(p.slots).map((s) => String(s.id))),
      ...programs.flatMap((p) => p.slots.map((s) => s.id)),
      ...list(toJs(file('package.yaml')?.text)?.migrations).flatMap((m) => [String((m.retire as Js | undefined)?.id ?? ''), String((m.rename as Js | undefined)?.from ?? '')])
    ]);
    let base = name.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_+|_+$/g, '');
    if (!/^[a-z]/.test(base)) base = `slot_${base}`.replace(/_+$/, '');
    let id = base;
    for (let n = 2; used.has(id) || texts[`s.${id}`] !== undefined; n++) id = `${base}_${n}`;
    return id;
  }

  function addSlot(p: Prog) {
    const name = p.adding.trim();
    if (!name) return;
    const id = slugFor(name);
    p.slots.push({ j: null, id, key: `s.${id}`, label: name, enabled: true, count: 1, repeated: false, mode: 'program', removed: false });
    p.adding = '';
  }

  async function removeSlot(p: Prog, s: Slot) {
    if (s.j !== null && !(await ask(t('sch_remove_confirm').replace('{name}', s.label), t('g_remove'), true))) return;
    if (s.j === null) p.slots.splice(p.slots.indexOf(s), 1);
    else s.removed = true;
  }

  function files(): ConfigFile[] | null {
    if (!current) return null;
    const cal = parseDoc(file('calendar.yaml')?.text ?? '');
    const prog = parseDoc(file('programs.yaml')?.text ?? '');
    const pkg = parseDoc(file('package.yaml')?.text ?? '');
    const textsPath = `texts/${locale}.yaml`;
    const txt = parseDoc(file(textsPath)?.text ?? '');
    if (!cal || !prog || !pkg || !txt) return null;
    for (const e of events) {
      setAt(cal, ['event_types', e.i, 'default_recurrence'], `FREQ=WEEKLY;BYDAY=${e.days.join(',')}`);
      setAt(cal, ['event_types', e.i, 'default_time'], e.time || undefined);
    }
    for (const o of options) setAt(prog, ['program_types', o.i, 'options', o.k, 'enabled'], o.enabled ? true : undefined);
    for (const p of programs) {
      const base = ['program_types', p.i];
      setAt(prog, [...base, 'rotation'], p.rotation === 'occurrence' ? undefined : p.rotation);
      setAt(prog, [...base, 'assign_units'], p.units || undefined);
      if (p.rotation === 'month') setAt(prog, [...base, 'period'], 'month');
      for (const s of p.slots) {
        if (s.label.trim() && s.label !== texts[s.key]) setText(txt, s.key, s.label.trim());
        if (s.j === null) {
          if (s.removed) continue;
          const slot: Js = { id: s.id, label: s.key, count: s.count };
          if (p.requires !== undefined) slot.requires = p.requires;
          if (!s.enabled) slot.enabled = false;
          if (s.mode !== 'program') slot.mode = s.mode;
          addTo(prog, [...base, 'slots'], slot);
          continue;
        }
        setAt(prog, [...base, 'slots', s.j, 'enabled'], s.enabled ? undefined : false);
        setAt(prog, [...base, 'slots', s.j, 'each_event'], undefined);
        setAt(prog, [...base, 'slots', s.j, 'mode'], s.mode === 'program' ? undefined : s.mode);
        if (!s.repeated) setAt(prog, [...base, 'slots', s.j, 'count'], s.count);
      }
      // Removals last, from the end, so the positions above stay valid.
      for (const s of [...p.slots].filter((x) => x.removed && x.j !== null).sort((a, b) => b.j! - a.j!)) {
        prog.deleteIn([...base, 'slots', s.j!]);
      }
    }
    // A slot id that no program has any more is declared retired (§6.6).
    const left = new Set(list(prog.toJS()?.program_types).flatMap((p) => list(p.slots).map((s) => String(s.id))));
    for (const s of programs.flatMap((p) => p.slots).filter((x) => x.removed && x.j !== null)) {
      if (!left.has(s.id)) addRetire(pkg, s.id);
    }
    const out: Record<string, string> = {
      'calendar.yaml': toText(cal),
      'programs.yaml': toText(prog),
      'package.yaml': toText(pkg),
      [textsPath]: toText(txt)
    };
    return current.files.map((f) => (f.path in out ? { ...f, text: out[f.path] } : { ...f }));
  }

  async function save() {
    if (!current) return;
    issues = [];
    // Another device or page may have published meanwhile: start from that version.
    try {
      const latest = await run(api.configFiles());
      if (latest.version !== current.version) {
        toast(t('sch_stale').replace('{n}', String(latest.version)), true);
        await load();
        return;
      }
    } catch (e) {
      return fail(e);
    }
    const fs = files();
    if (!fs) return toast(t('sch_syntax'), true);
    if (programs.some((p) => !p.slots.some((s) => s.enabled && !s.removed))) return toast(t('sch_none'), true);
    if (events.some((e) => !e.days.length)) return toast(t('sch_no_days'), true);
    if (programs.some((p) => p.slots.some((s) => !s.removed && !s.label.trim()))) return toast(t('sch_no_name'), true);
    try {
      issues = (await run(api.configCheck(fs))).filter((i) => i.severity === 'error');
    } catch (e) {
      return fail(e);
    }
    if (issues.length) return;
    if (!(await ask(t('config_confirm').replace('{n}', String(current.version + 1)), t('sch_save')))) return;
    busy = true;
    try {
      const n = await run(api.configPublish(fs));
      toast(t('sch_saved').replace('{n}', String(n)));
      await load();
      onchange();
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if current}
  <div class="stack page">
    <header class="stack tight">
      <h1>{t('sch_title')}</h1>
      <p class="muted">{current.can_publish ? t('sch_hint') : seriesList?.event_types.length ? t('sch_series_hint') : t('sch_readonly')}</p>
    </header>

    {#if (current.can_publish && events.length) || seriesList?.event_types.length}
      <section class="stack" aria-labelledby="sch-events">
        <h2 id="sch-events">{seriesList?.title ?? t('sch_events')}</h2>
        {#if current.can_publish}
          {#each events as e (e.id)}
            <div class="card stack">
              <h3>{e.label}</h3>
              <div class="row wrap">
                <div class="days" role="group" aria-label={`${e.label} · ${t('sch_days')}`}>
                  {#each DAYS as d (d)}
                    <button class="day" aria-pressed={e.days.includes(d)} disabled={!current.can_publish} onclick={() => toggleDay(e, d)}>{t(`wd_${d}` as Key)}</button>
                  {/each}
                </div>
                <label class="inline time">{t('sch_time')} <input type="time" bind:value={e.time} disabled={!current.can_publish} /></label>
              </div>
              <!-- The options of the programs held at this event (e.g. an auxiliary class). -->
              {#each options.filter((o) => o.events.includes(e.id)) as o (o.id)}
                <label class="inline option">
                  <input type="checkbox" bind:checked={o.enabled} disabled={!current.can_publish} />
                  <span>{o.label}</span>
                </label>
              {/each}
            </div>
          {/each}
        {/if}
        {#each seriesList?.event_types ?? [] as et (et.id)}
          <!-- Weekly series of this kind, by day: grey a day without, white a day with, the chosen one in colour. -->
          {@const day = dayOf(et.id)}
          {@const rows = seriesRows.filter((m) => m.event_type === et.id && m.weekday === day)}
          <div class="card stack">
            <h3>{et.label}</h3>
            <div class="days" role="group" aria-label={`${et.label} · ${t('sch_days')}`}>
              {#each DAYS as d, k (d)}
                <button class="day" class:none={!seriesRows.some((m) => m.event_type === et.id && m.weekday === k + 1)}
                  aria-pressed={day === k + 1} onclick={() => (selDay[et.id] = k + 1)}>{t(`wd_${d}` as Key)}</button>
              {/each}
            </div>
            {#each rows as m, i (m.id ?? `new-${i}`)}
              <div class="meet stack">
                <div class="fields">
                  <label class="field"><span class="muted">{t('sch_time')}</span><input type="time" bind:value={m.time} /></label>
                  <label class="field grow"><span class="muted">{t('sch_place')}</span><input class="place" bind:value={m.location} /></label>
                </div>
                <div class="field">
                  <span class="muted" id={`who-${et.id}-${i}`}>{t('sch_conductor')}</span>
                  <div class="seg" role="radiogroup" aria-labelledby={`who-${et.id}-${i}`}>
                    <button role="radio" aria-checked={!m.conducted_by_unit} onclick={() => (m.conducted_by_unit = false)}><Icon name="profile" size={18} />{t('sch_conductor_person')}</button>
                    <button role="radio" aria-checked={m.conducted_by_unit} onclick={() => (m.conducted_by_unit = true)}><Icon name="work_queue" size={18} />{t('sch_conductor_unit')}</button>
                  </div>
                  <span class="muted small">{m.conducted_by_unit ? t('sch_conductor_unit_hint') : t('sch_conductor_person_hint')}</span>
                </div>
                <div class="field">
                  <span class="muted">{t('sch_groups')}</span>
                  <div class="row wrap units" role="group" aria-label={t('sch_groups')}>
                    {#each seriesList?.units ?? [] as u (u.id)}
                      <button class="day" aria-pressed={m.units.includes(u.id)} onclick={() => toggleUnit(m, u.id)}>{u.label}</button>
                    {/each}
                    {#if unified(m)}<span class="badge ok">{t('sch_unified')}</span>{/if}
                  </div>
                </div>
                <div class="row wrap actions">
                  <button class="btn danger-outline" onclick={() => removeSeries(m)}>
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M3 6h18 M8 6V4h8v2 M6 6l1 14h10l1-14" /></svg>
                    {t('g_remove')}
                  </button>
                  {#if rowKey(m) !== m.saved}
                    <button class="btn primary" disabled={!m.location.trim() || !m.time || !m.units.length} onclick={() => saveSeries(m)}>{t('save_plain')}</button>
                  {/if}
                </div>
              </div>
            {:else}
              <p class="muted">{t('sch_series_none')}</p>
            {/each}
            <div class="row"><button class="btn" onclick={() => addSeries(et.id, day)}>{t('sch_series_add')}</button></div>
          </div>
        {/each}
      </section>
    {/if}

    {#if current.can_publish && options.some((o) => !events.some((e) => o.events.includes(e.id)))}
      <section class="stack" aria-labelledby="sch-options">
        <h2 id="sch-options">{t('sch_options')}</h2>
        <div class="card stack">
          {#each options.filter((o) => !events.some((e) => o.events.includes(e.id))) as o (o.id)}
            <label class="inline option">
              <input type="checkbox" bind:checked={o.enabled} disabled={!current.can_publish} />
              <span>{o.label}</span>
              <span class="muted">· {o.program}</span>
            </label>
          {/each}
        </div>
      </section>
    {/if}

    {#if current.can_publish && programs.length}
      <section class="stack" aria-labelledby="sch-programs">
        <h2 id="sch-programs">{t('sch_programs')}</h2>
        {#each programs as p (p.id)}
          <div class="card stack">
            <div class="spread wrap">
              <div class="row title">
                <h3>{p.label}</h3>
                <!-- To persons or to groups (units of the first unit type): any group may take a slot. -->
                <button class="who-btn" class:on={!!p.units} disabled={!current.can_publish || !unitType}
                  title={p.units ? t('sch_to_units') : t('sch_to_persons')} aria-label={p.units ? t('sch_to_units') : t('sch_to_persons')} aria-pressed={!!p.units}
                  onclick={() => (p.units = p.units ? '' : unitType)}>
                  <Icon name={p.units ? 'work_queue' : 'profile'} size={20} />
                </button>
              </div>
              <label class="inline">
                <span class="muted">{t('sch_rotation')}</span>
                <select bind:value={p.rotation} disabled={!current.can_publish} aria-label={`${p.label} · ${t('sch_rotation')}`}>
                  <option value="occurrence">{t('rot_occurrence')}</option>
                  <option value="week">{t('rot_week')}</option>
                  <option value="month">{t('rot_month')}</option>
                </select>
              </label>
            </div>
            <div class="stack slots" role="group" aria-label={`${p.label} · ${t('sch_slots')}`}>
              <span class="muted small">{t('sch_slots')}</span>
              {#each p.slots.filter((x) => !x.removed) as s (s.id)}
                <div class="slot" class:off={!s.enabled}>
                  <input class="on" type="checkbox" bind:checked={s.enabled} disabled={!current.can_publish} aria-label={`${s.label} · ${t('sch_active')}`} />
                  <input class="name" bind:value={s.label} disabled={!current.can_publish} aria-label={t('sch_name')} />
                  {#if !s.repeated}
                    <label class="inline count">
                      <span class="muted">{t(p.units ? 'sch_count_units' : 'sch_count')}</span>
                      <input type="number" min="1" max="9" bind:value={s.count} disabled={!current.can_publish || !s.enabled} aria-label={`${s.label} · ${t(p.units ? 'sch_count_units' : 'sch_count')}`} />
                    </label>
                  {/if}
                  <select class="each" bind:value={s.mode} disabled={!current.can_publish || !s.enabled} aria-label={`${s.label} · ${t('sch_mode')}`} title={t('sch_mode_hint')}>
                    {#each SLOT_MODES as m (m)}<option value={m}>{t(`sch_mode_${m}` as never)}</option>{/each}
                  </select>
                  {#if current.can_publish}
                    <button class="icon-btn" aria-label={`${t('g_remove')} ${s.label}`} onclick={() => removeSlot(p, s)}>
                      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M3 6h18 M8 6V4h8v2 M6 6l1 14h10l1-14" /></svg>
                    </button>
                  {/if}
                </div>
              {/each}
              {#if current.can_publish}
                <form class="row add" onsubmit={(e) => { e.preventDefault(); addSlot(p); }}>
                  <input class="grow" bind:value={p.adding} placeholder={t('sch_new_slot')} aria-label={`${p.label} · ${t('sch_new_slot')}`} />
                  <button class="btn" type="submit" disabled={!p.adding.trim()}>{t('add')}</button>
                </form>
              {/if}
            </div>
          </div>
        {/each}
      </section>
    {/if}

    {#if issues.length}
      <section class="card stack issues" role="alert">
        <h3>{t('sch_issues')}</h3>
        {#each issues as i, n (n)}<p><strong>{i.file}</strong>{i.path ? ` · ${i.path}` : ''} — {i.message}</p>{/each}
      </section>
    {/if}

    {#if current.can_publish}
      <div class="row">
        <button class="btn" disabled={busy || !dirty} onclick={load}>{t('config_discard')}</button>
        <button class="btn primary" disabled={busy || !dirty} onclick={save}>{t('sch_save')}</button>
      </div>

      <section class="card stack" aria-labelledby="sch-file">
        <h2 id="sch-file">{t('sch_file')}</h2>
        <p class="muted">{t('sch_file_hint')}</p>
        <div class="row wrap">
          <select class="format" bind:value={fileFormat} aria-label={t('sch_format')}>
            <option value="json">JSON</option>
            <option value="yaml">YAML</option>
          </select>
          <button class="btn" disabled={busy || dirty} onclick={exportFile}>{t('sch_export')}</button>
          <label class="btn" class:disabled={busy}>
            {t('sch_import')}
            <input type="file" accept=".json,.yaml,.yml,application/json,application/yaml" hidden disabled={busy} onchange={(e) => importFile(e.currentTarget)} />
          </label>
        </div>
      </section>
    {/if}
  </div>
{/if}

<style>
  .page { gap: 28px; }
  .format { width: auto; }
  .tight { gap: 4px; }
  .wrap { flex-wrap: wrap; gap: 12px; }
  .days { display: flex; gap: 6px; flex-wrap: wrap; }
  .day {
    min-width: 48px; min-height: 40px; padding: 6px 10px; border-radius: 10px; border: 1px solid var(--line);
    background: var(--bg); color: var(--text); font: inherit; font-size: 15px; cursor: pointer;
  }
  .day[aria-pressed='true'] { background: var(--primary); border-color: var(--primary); color: var(--on-primary); font-weight: 600; }
  .day:disabled { cursor: default; }
  .inline { display: flex; align-items: center; gap: 10px; flex-wrap: nowrap; font-weight: 500; }
  .inline select, .time input { width: auto; }
  .count input { width: 72px; }
  .small { font-size: 0.85rem; }
  .issues { border-color: var(--danger); }
  .issues p { font-size: 14px; }
  .slots { gap: 0; }
  .option { gap: 10px; }
  .option input { width: 18px; height: 18px; min-height: 0; margin: 0; }
  .slot { display: grid; grid-template-columns: auto minmax(140px, 1fr) auto auto auto; align-items: center; gap: 12px; padding: 8px 0; border-bottom: 1px solid var(--line); }
  .slot.off .name, .slot.off .count, .slot.off .each { opacity: 0.5; }
  .on { width: 20px; height: 20px; min-height: 20px; margin: 0; accent-color: var(--primary); flex: none; }
  .name { min-height: 38px; padding: 6px 10px; }
  .each { font-size: 14px; width: auto; min-width: 200px; min-height: 38px; }
  .icon-btn { display: grid; place-items: center; width: 36px; height: 36px; border: none; border-radius: 8px; background: none; color: var(--muted); cursor: pointer; }
  .icon-btn:hover { background: var(--surface); color: var(--danger); }
  .add { margin-top: 12px; flex-wrap: nowrap; }
  .meet { gap: 14px; padding: 14px 0; border-top: 1px solid var(--line); }
  .day.none:not([aria-pressed='true']) { background: var(--page); color: var(--muted); border-color: transparent; }
  .fields { display: flex; flex-wrap: wrap; gap: 12px; }
  .field { display: flex; flex-direction: column; gap: 6px; font-weight: 500; }
  .field.grow { flex: 1 1 220px; }
  .field input[type='time'] { width: auto; }
  .seg { display: inline-flex; border: 1px solid var(--border); border-radius: 10px; overflow: hidden; width: fit-content; }
  .seg button { display: flex; align-items: center; gap: 6px; min-height: 40px; padding: 6px 14px; border: 0; background: var(--bg); color: var(--text); font: inherit; font-size: 15px; cursor: pointer; }
  .seg button + button { border-left: 1px solid var(--border); }
  .seg button[aria-checked='true'] { background: var(--primary); color: var(--on-primary); font-weight: 600; }
  .actions { justify-content: space-between; }
  .danger-outline { display: inline-flex; align-items: center; gap: 6px; color: var(--danger); border-color: var(--danger); }
  .units { gap: 6px; align-items: center; }
  .title { gap: 10px; align-items: center; flex-wrap: nowrap; }
  .title h3 { margin: 0; }
  .who-btn { display: grid; place-items: center; width: 36px; height: 36px; border-radius: 10px; border: 1px solid var(--border); background: var(--bg); color: var(--muted); cursor: pointer; padding: 0; }
  .who-btn.on { color: var(--primary-strong); background: var(--primary-soft); border-color: transparent; }
  .who-btn:hover:not(:disabled) { border-color: var(--primary); color: var(--primary); }
  @media (max-width: 640px) {
    .slot { grid-template-columns: auto 1fr auto; }
    .slot .count, .slot .each { grid-column: 2 / 4; }
  }
</style>
