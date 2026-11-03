<script lang="ts">
  import { ask } from '../ask.svelte';
  // Calendar and rotations: the days of the recurring events and, for the
  // programs that cover several events, how long the same persons keep a
  // slot and which slots are assigned. It edits calendar.yaml and
  // programs.yaml like the guided editors and publishes a new version.
  import type { ConfigFile, ConfigIssue, ConfigVersion_Serialize as ConfigVersion } from '../bindings';
  import { api, run } from '../api';
  import { t, type Key } from '../i18n';
  import { fail, toast } from '../toast.svelte';
  import { parseDoc, toText, toJs, setAt, addTo, addRetire, setText, flatTexts, type Js } from '../yamlEdit';

  let { onchange }: { onchange: () => void } = $props();

  const DAYS = ['MO', 'TU', 'WE', 'TH', 'FR', 'SA', 'SU'];
  type Ev = { i: number; id: string; label: string; days: string[]; time: string };
  // j: position in programs.yaml (null for a slot added here).
  type Slot = { j: number | null; id: string; key: string; label: string; enabled: boolean; count: number; repeated: boolean; eachEvent: boolean; removed: boolean };
  type Prog = { i: number; id: string; label: string; rotation: string; period: string; slots: Slot[]; requires: unknown; adding: string };
  // An option of a program type (e.g. a second room): i program, k option.
  type Opt = { i: number; k: number; id: string; program: string; label: string; enabled: boolean };

  let current = $state<ConfigVersion | null>(null);
  let events = $state<Ev[]>([]);
  let programs = $state<Prog[]>([]);
  let options = $state<Opt[]>([]);
  let busy = $state(false);
  let issues = $state<ConfigIssue[]>([]);
  let initial = '';
  let locale = 'it';
  let texts: Record<string, string> = {};

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
          requires: list(p.slots)[0]?.requires,
          adding: '',
          slots: list(p.slots).map((s, j) => ({
            j,
            id: String(s.id),
            key: String(s.label),
            label: label(s.label),
            enabled: s.enabled !== false,
            count: Number(s.count ?? 1),
            repeated: s.repeat !== undefined,
            eachEvent: s.each_event === true,
            removed: false
          }))
        }));
      options = list(prog?.program_types).flatMap((p, i) =>
        list(p.options).map((o, k) => ({ i, k, id: `${String(p.id)}.${String(o.id)}`, program: label(p.label), label: label(o.label), enabled: o.enabled === true }))
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
    p.slots.push({ j: null, id, key: `s.${id}`, label: name, enabled: true, count: 1, repeated: false, eachEvent: false, removed: false });
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
      if (p.rotation === 'month') setAt(prog, [...base, 'period'], 'month');
      for (const s of p.slots) {
        if (s.label.trim() && s.label !== texts[s.key]) setText(txt, s.key, s.label.trim());
        if (s.j === null) {
          if (s.removed) continue;
          const slot: Js = { id: s.id, label: s.key, count: s.count };
          if (p.requires !== undefined) slot.requires = p.requires;
          if (!s.enabled) slot.enabled = false;
          if (s.eachEvent) slot.each_event = true;
          addTo(prog, [...base, 'slots'], slot);
          continue;
        }
        setAt(prog, [...base, 'slots', s.j, 'enabled'], s.enabled ? undefined : false);
        setAt(prog, [...base, 'slots', s.j, 'each_event'], s.eachEvent ? true : undefined);
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
      <p class="muted">{current.can_publish ? t('sch_hint') : t('sch_readonly')}</p>
    </header>

    {#if events.length}
      <section class="stack" aria-labelledby="sch-events">
        <h2 id="sch-events">{t('sch_events')}</h2>
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
          </div>
        {/each}
      </section>
    {/if}

    {#if options.length}
      <section class="stack" aria-labelledby="sch-options">
        <h2 id="sch-options">{t('sch_options')}</h2>
        <div class="card stack">
          {#each options as o (o.id)}
            <label class="inline option">
              <input type="checkbox" bind:checked={o.enabled} disabled={!current.can_publish} />
              <span>{o.label}</span>
              <span class="muted">· {o.program}</span>
            </label>
          {/each}
        </div>
      </section>
    {/if}

    {#if programs.length}
      <section class="stack" aria-labelledby="sch-programs">
        <h2 id="sch-programs">{t('sch_programs')}</h2>
        {#each programs as p (p.id)}
          <div class="card stack">
            <div class="spread wrap">
              <h3>{p.label}</h3>
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
                      <span class="muted">{t('sch_count')}</span>
                      <input type="number" min="1" max="9" bind:value={s.count} disabled={!current.can_publish || !s.enabled} aria-label={`${s.label} · ${t('sch_count')}`} />
                    </label>
                  {/if}
                  <label class="inline each" title={t('sch_each_hint')}>
                    <input type="checkbox" bind:checked={s.eachEvent} disabled={!current.can_publish || !s.enabled || p.rotation === 'occurrence'} />
                    <span>{t('sch_each')}</span>
                  </label>
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
    {/if}
  </div>
{/if}

<style>
  .page { gap: 28px; }
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
  .on, .each input { width: 20px; height: 20px; min-height: 20px; margin: 0; accent-color: var(--primary); flex: none; }
  .name { min-height: 38px; padding: 6px 10px; }
  .each { font-size: 14px; white-space: nowrap; }
  .icon-btn { display: grid; place-items: center; width: 36px; height: 36px; border: none; border-radius: 8px; background: none; color: var(--muted); cursor: pointer; }
  .icon-btn:hover { background: var(--surface); color: var(--danger); }
  .add { margin-top: 12px; flex-wrap: nowrap; }
  @media (max-width: 640px) {
    .slot { grid-template-columns: auto 1fr auto; }
    .slot .count, .slot .each { grid-column: 2 / 4; }
  }
</style>
