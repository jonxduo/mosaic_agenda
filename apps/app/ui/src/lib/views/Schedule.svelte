<script lang="ts">
  // Calendar and rotations: the days of the recurring events and, for the
  // programs that cover several events, how long the same persons keep a
  // slot and which slots are assigned. It edits calendar.yaml and
  // programs.yaml like the guided editors and publishes a new version.
  import type { ConfigFile, ConfigVersion_Serialize as ConfigVersion } from '../bindings';
  import { api, run } from '../api';
  import { t, type Key } from '../i18n';
  import { fail, toast } from '../toast.svelte';
  import { parseDoc, toText, toJs, setAt, flatTexts, type Js } from '../yamlEdit';

  let { onchange }: { onchange: () => void } = $props();

  const DAYS = ['MO', 'TU', 'WE', 'TH', 'FR', 'SA', 'SU'];
  type Ev = { i: number; id: string; label: string; days: string[]; time: string };
  type Slot = { j: number; id: string; label: string; enabled: boolean; count: number; repeated: boolean };
  type Prog = { i: number; id: string; label: string; rotation: string; period: string; slots: Slot[] };

  let current = $state<ConfigVersion | null>(null);
  let events = $state<Ev[]>([]);
  let programs = $state<Prog[]>([]);
  let busy = $state(false);
  let initial = '';

  const list = (v: unknown): Js[] => (Array.isArray(v) ? (v as Js[]) : []);
  const file = (path: string) => current?.files.find((f) => f.path === path);
  const snapshot = () => JSON.stringify({ events, programs });
  const dirty = $derived(current !== null && snapshot() !== initial);

  async function load() {
    try {
      current = await run(api.configFiles());
      const pkg = toJs(file('package.yaml')?.text);
      const locale = String(pkg?.default_locale ?? 'it');
      const texts = flatTexts(toJs(file(`texts/${locale}.yaml`)?.text));
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
          slots: list(p.slots).map((s, j) => ({
            j,
            id: String(s.id),
            label: label(s.label),
            enabled: s.enabled !== false,
            count: Number(s.count ?? 1),
            repeated: s.repeat !== undefined
          }))
        }));
      initial = snapshot();
    } catch (e) {
      fail(e);
    }
  }
  load();

  const toggleDay = (e: Ev, d: string) => (e.days = e.days.includes(d) ? e.days.filter((x) => x !== d) : DAYS.filter((x) => x === d || e.days.includes(x)));

  function files(): ConfigFile[] | null {
    if (!current) return null;
    const cal = parseDoc(file('calendar.yaml')?.text ?? '');
    const prog = parseDoc(file('programs.yaml')?.text ?? '');
    if (!cal || !prog) return null;
    for (const e of events) {
      setAt(cal, ['event_types', e.i, 'default_recurrence'], `FREQ=WEEKLY;BYDAY=${e.days.join(',')}`);
      setAt(cal, ['event_types', e.i, 'default_time'], e.time || undefined);
    }
    for (const p of programs) {
      setAt(prog, ['program_types', p.i, 'rotation'], p.rotation === 'occurrence' ? undefined : p.rotation);
      if (p.rotation === 'month') setAt(prog, ['program_types', p.i, 'period'], 'month');
      for (const s of p.slots) {
        setAt(prog, ['program_types', p.i, 'slots', s.j, 'enabled'], s.enabled ? undefined : false);
        if (!s.repeated) setAt(prog, ['program_types', p.i, 'slots', s.j, 'count'], s.count);
      }
    }
    return current.files.map((f) =>
      f.path === 'calendar.yaml' ? { ...f, text: toText(cal) } : f.path === 'programs.yaml' ? { ...f, text: toText(prog) } : { ...f }
    );
  }

  async function save() {
    const fs = files();
    if (!fs || !current) return;
    if (programs.some((p) => !p.slots.some((s) => s.enabled))) return fail(new Error(t('sch_none')));
    if (!confirm(t('config_confirm').replace('{n}', String(current.version + 1)))) return;
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
              {#each p.slots as s (s.id)}
                <div class="list-row">
                  <label class="check grow"><input type="checkbox" bind:checked={s.enabled} disabled={!current.can_publish} /> {s.label}</label>
                  {#if !s.repeated}
                    <label class="inline count" class:off={!s.enabled}>
                      <span class="muted">{t('sch_count')}</span>
                      <input type="number" min="1" max="9" bind:value={s.count} disabled={!current.can_publish || !s.enabled} aria-label={`${s.label} · ${t('sch_count')}`} />
                    </label>
                  {/if}
                </div>
              {/each}
            </div>
          </div>
        {/each}
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
  .check { display: flex; align-items: center; gap: 12px; font-weight: 500; min-height: 44px; }
  .check input { width: 20px; height: 20px; min-height: 20px; margin: 0; accent-color: var(--primary); }
  .count.off { opacity: 0.5; }
  .small { font-size: 0.85rem; }
  .slots { gap: 0; }
</style>
