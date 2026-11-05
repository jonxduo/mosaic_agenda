<script lang="ts">
  import { ask } from '../ask.svelte';
  // Guided editors of the configuration (§12.4): roles and their
  // requirements, audiences, form fields, program slots, reminders and texts.
  // They write the same YAML files as the text view: conditions and lists are
  // edited as one line of YAML. Changing or removing an id that the active
  // version already had adds the migration to package.yaml.
  import type { ConfigFile } from '../bindings';
  import type { Document } from 'yaml';
  import { t } from '../i18n';
  import { toast } from '../toast.svelte';
  import { parseDoc, toText, toJs, at, setAt, addTo, snippet, fromSnippet, addRename, addRetire, flatTexts, setText, type Path, type Js } from '../yamlEdit';

  let {
    files,
    original,
    readonly,
    onedit
  }: { files: ConfigFile[]; original: (path: string) => string | undefined; readonly: boolean; onedit: (path: string, text: string) => void } = $props();

  type Kind = 'id' | 'text' | 'number' | 'bool' | 'select' | 'yaml' | 'label';
  type Group = { title: string; list: Path; items: Js[] };
  type Spec = { key: Path; label: string; kind: Kind; options?: (g: Group) => string[]; placeholder?: string };
  type Section = {
    id: string;
    title: string;
    file: string;
    specs: Spec[];
    groups: Group[];
    /** Ids that need a migration when they disappear (§6.6). */
    ids?: (js: Js | null) => string[];
    fresh: (id: string) => Js;
  };

  const SYSTEM_AUDIENCES = ['subject_only', 'subject_or_on_behalf', 'subject_missing', 'assignee', 'resource_holder', 'object_team', 'root_admins'];
  const FIELD_TYPES = ['integer', 'decimal', 'duration', 'boolean', 'text', 'long_text', 'date', 'period', 'choice', 'multi_choice', 'person', 'resource', 'photos', 'geo', 'computed'];
  const REMINDER_KINDS = ['document.missing_after', 'batch.incomplete_after', 'batch.missing_after', 'event.assignment_before', 'resource.no_log_after', 'request.due_after'];
  const SLUG = /^[a-z][a-z0-9_]*$/;

  const js = $derived(Object.fromEntries(files.map((f) => [f.path, toJs(f.text)])) as Record<string, Js | null>);
  const listOf = (j: Js | null | undefined, path: Path): Js[] => {
    const v = at(j, path);
    return Array.isArray(v) ? (v as Js[]) : [];
  };
  const idsOf = (items: Js[]) => items.map((i) => String(i.id ?? ''));
  const slotIds = (j: Js | null) => listOf(j, ['program_types']).flatMap((p) => idsOf(listOf(p, ['slots'])));

  const roles = $derived(idsOf(listOf(js['roles.yaml'], ['roles'])));
  const audiences = $derived([...idsOf(listOf(js['audiences.yaml'], ['audiences'])), ...SYSTEM_AUDIENCES]);
  const unitTypes = $derived(idsOf(listOf(js['units.yaml'], ['unit_types'])));

  const locales = $derived(files.filter((f) => /^texts\/[^/]+\.yaml$/.test(f.path)).map((f) => f.path.slice(6, -5)));
  let chosenLocale = $state('');
  const locale = $derived(locales.includes(chosenLocale) ? chosenLocale : String(js['package.yaml']?.default_locale ?? locales[0] ?? ''));
  const textsPath = $derived(`texts/${locale}.yaml`);
  const texts = $derived(flatTexts(js[textsPath] ?? null));

  const sections = $derived<Section[]>([
    {
      id: 'roles',
      title: t('g_roles'),
      file: 'roles.yaml',
      groups: [{ title: '', list: ['roles'], items: listOf(js['roles.yaml'], ['roles']) }],
      ids: (j) => idsOf(listOf(j, ['roles'])),
      fresh: (id) => ({ id, label: `r.${id}`, scope: 'organization', holders: { min: 0, max: 1 } }),
      specs: [
        { key: ['id'], label: t('g_id'), kind: 'id' },
        { key: ['label'], label: t('g_label'), kind: 'label' },
        { key: ['scope'], label: t('g_scope'), kind: 'select', options: () => ['organization', ...unitTypes.map((u) => `unit:${u}`)] },
        { key: ['requires'], label: t('g_requires'), kind: 'yaml', placeholder: '{ has_qualification: … }' },
        { key: ['holders', 'min'], label: t('g_holders_min'), kind: 'number' },
        { key: ['holders', 'max'], label: t('g_holders_max'), kind: 'number', placeholder: t('g_unlimited') },
        { key: ['priority'], label: t('g_priority'), kind: 'number' },
        { key: ['permissions'], label: t('g_permissions'), kind: 'yaml', placeholder: '[ { permission: …, on: …, target: … } ]' },
        { key: ['substitute_for'], label: t('g_substitute_for'), kind: 'select', options: () => roles },
        { key: ['assistant_of'], label: t('g_assistant_of'), kind: 'select', options: () => roles },
        { key: ['supervises'], label: t('g_supervises'), kind: 'select', options: () => roles },
        { key: ['exclusive_with'], label: t('g_exclusive_with'), kind: 'yaml', placeholder: '[ … ]' },
        { key: ['mandatory'], label: t('g_mandatory'), kind: 'yaml', placeholder: '{ always: true }' }
      ]
    },
    {
      id: 'audiences',
      title: t('g_audiences'),
      file: 'audiences.yaml',
      groups: [{ title: '', list: ['audiences'], items: listOf(js['audiences.yaml'], ['audiences']) }],
      ids: (j) => idsOf(listOf(j, ['audiences'])),
      fresh: (id) => ({ id, select: { subject: true } }),
      specs: [
        { key: ['id'], label: t('g_id'), kind: 'id' },
        { key: ['select'], label: t('g_select'), kind: 'yaml', placeholder: '{ union: [ … ] }' }
      ]
    },
    {
      id: 'fields',
      title: t('g_fields'),
      file: 'forms.yaml',
      groups: listOf(js['forms.yaml'], ['forms']).flatMap((f, fi) =>
        listOf(f, ['variants']).map((v, vi) => ({
          title: `${f.id} · ${v.id}`,
          list: ['forms', fi, 'variants', vi, 'fields'],
          items: listOf(v, ['fields'])
        }))
      ),
      fresh: (id) => ({ id, type: 'text', label: `fld.${id}` }),
      specs: [
        { key: ['id'], label: t('g_id'), kind: 'id' },
        { key: ['type'], label: t('g_type'), kind: 'select', options: () => FIELD_TYPES },
        { key: ['label'], label: t('g_label'), kind: 'label' },
        { key: ['required'], label: t('g_required'), kind: 'bool' },
        { key: ['min'], label: t('g_min'), kind: 'number' },
        { key: ['max'], label: t('g_max'), kind: 'number' },
        { key: ['max_length'], label: t('g_max_length'), kind: 'number' },
        { key: ['options'], label: t('g_options'), kind: 'yaml', placeholder: '[ a, b ]' }
      ]
    },
    {
      id: 'slots',
      title: t('g_slots'),
      file: 'programs.yaml',
      groups: listOf(js['programs.yaml'], ['program_types']).map((p, pi) => ({
        title: String(p.id ?? ''),
        list: ['program_types', pi, 'slots'],
        items: listOf(p, ['slots'])
      })),
      ids: slotIds,
      fresh: (id) => ({ id, label: `s.${id}` }),
      specs: [
        { key: ['id'], label: t('g_id'), kind: 'id' },
        { key: ['label'], label: t('g_label'), kind: 'label' },
        { key: ['requires'], label: t('g_requires'), kind: 'yaml', placeholder: '{ has_qualification: … }' },
        { key: ['count'], label: t('g_count'), kind: 'number', placeholder: '1' },
        { key: ['repeat', 'count'], label: t('g_repeat'), kind: 'number' },
        { key: ['allow_external'], label: t('g_allow_external'), kind: 'bool' },
        { key: ['pair_with'], label: t('g_pair_with'), kind: 'select', options: (g) => idsOf(g.items) },
        { key: ['extra_fields'], label: t('g_extra_fields'), kind: 'yaml', placeholder: '[ { id: title, type: text } ]' }
      ]
    },
    {
      id: 'reminders',
      title: t('g_reminders'),
      file: 'notifications.yaml',
      groups: [{ title: '', list: ['reminders'], items: listOf(js['notifications.yaml'], ['reminders']) }],
      fresh: (id) => ({ id, kind: 'event.assignment_before', days: 2, audience: 'assignee', text: `n.${id}` }),
      specs: [
        { key: ['id'], label: t('g_id'), kind: 'id' },
        { key: ['kind'], label: t('g_kind'), kind: 'select', options: () => REMINDER_KINDS },
        { key: ['days'], label: t('g_days'), kind: 'number' },
        { key: ['audience'], label: t('g_audience'), kind: 'select', options: () => audiences },
        { key: ['text'], label: t('g_text'), kind: 'label' },
        { key: ['category'], label: t('g_category'), kind: 'text' },
        { key: ['mandatory'], label: t('g_mandatory_notice'), kind: 'bool' },
        { key: ['filter'], label: t('g_filter'), kind: 'yaml', placeholder: '{ form: … }' }
      ]
    }
  ]);

  let tab = $state('roles');
  let openItem = $state('');
  let newIds = $state<Record<string, string>>({});
  let textFilter = $state('');
  let newKey = $state('');
  let newValue = $state('');
  const section = $derived(sections.find((s) => s.id === tab));

  const textOf = (path: string) => files.find((f) => f.path === path)?.text;

  /** Applies a change to one file; false if the file is missing or has syntax errors. */
  function edit(path: string, fn: (doc: Document) => void): boolean {
    const text = textOf(path);
    const doc = text === undefined ? null : parseDoc(text);
    if (!doc) {
      toast(t('g_syntax').replace('{f}', path), true);
      return false;
    }
    try {
      fn(doc);
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), true);
      return false;
    }
    onedit(path, toText(doc));
    return true;
  }

  /** Whether `id` disappeared from the file but existed in the active version. */
  function needsMigration(s: Section, id: string): boolean {
    if (!s.ids || !id) return false;
    return s.ids(toJs(original(s.file))).includes(id) && !s.ids(toJs(textOf(s.file))).includes(id);
  }

  function shown(item: Js, sp: Spec): string {
    const v = at(item, sp.key);
    if (sp.kind === 'yaml') return snippet(v);
    return v === undefined || v === null ? '' : String(v);
  }

  function change(s: Section, g: Group, i: number, sp: Spec, raw: string | boolean) {
    let v: unknown;
    try {
      if (sp.kind === 'number') {
        v = raw === '' ? undefined : Number(raw);
        if (Number.isNaN(v)) throw new Error(t('g_bad_number'));
      } else if (sp.kind === 'bool') {
        v = raw ? true : undefined;
      } else if (sp.kind === 'yaml') {
        v = fromSnippet(String(raw));
      } else {
        v = String(raw).trim() || undefined;
      }
    } catch (e) {
      toast(`${sp.label}: ${e instanceof Error ? e.message : String(e)}`, true);
      return;
    }
    edit(s.file, (d) => setAt(d, [...g.list, i, ...sp.key], v));
  }

  function rename(s: Section, g: Group, i: number, from: string, raw: string) {
    const to = raw.trim();
    if (to === from) return;
    if (!SLUG.test(to)) return toast(t('g_bad_id'), true);
    if (!edit(s.file, (d) => d.setIn([...g.list, i, 'id'], to))) return;
    if (needsMigration(s, from)) edit('package.yaml', (d) => addRename(d, from, to));
    openItem = `${s.id}:${g.title}:${to}`;
  }

  async function remove(s: Section, g: Group, i: number, id: string) {
    if (!(await ask(t('g_remove_confirm').replace('{id}', id), t('g_remove'), true))) return;
    if (!edit(s.file, (d) => d.deleteIn([...g.list, i]))) return;
    if (needsMigration(s, id)) edit('package.yaml', (d) => addRetire(d, id));
  }

  function add(s: Section, g: Group) {
    const slot = `${s.id}:${g.title}`;
    const id = (newIds[slot] ?? '').trim();
    if (!SLUG.test(id)) return toast(t('g_bad_id'), true);
    if (edit(s.file, (d) => addTo(d, g.list, s.fresh(id)))) {
      newIds[slot] = '';
      openItem = `${slot}:${id}`;
    }
  }

  const setTextValue = (key: string, value: string) => edit(textsPath, (d) => setText(d, key, value));

  function addText() {
    const key = newKey.trim();
    if (!key || !newValue.trim()) return;
    if (setTextValue(key, newValue)) {
      newKey = newValue = '';
    }
  }

  const textRows = $derived(
    Object.entries(texts).filter(([k, v]) => !textFilter || k.includes(textFilter) || v.toLowerCase().includes(textFilter.toLowerCase()))
  );
  const titleOf = (item: Js) => {
    const label = item.label ?? item.text;
    return typeof label === 'string' && texts[label] ? texts[label] : '';
  };
</script>

<div class="stack guided">
  <div class="row tabs" role="tablist" aria-label={t('g_sections')}>
    {#each sections as s (s.id)}
      <button class="tab" role="tab" aria-selected={tab === s.id} class:active={tab === s.id} onclick={() => (tab = s.id)}>{s.title}</button>
    {/each}
    <button class="tab" role="tab" aria-selected={tab === 'texts'} class:active={tab === 'texts'} onclick={() => (tab = 'texts')}>{t('g_texts')}</button>
    {#if locales.length > 1}
      <select aria-label={t('g_locale')} value={locale} onchange={(e) => (chosenLocale = e.currentTarget.value)}>
        {#each locales as l (l)}<option value={l}>{l}</option>{/each}
      </select>
    {/if}
  </div>

  {#if tab === 'texts'}
    <section class="card stack">
      <input type="search" placeholder={t('g_search')} aria-label={t('g_search')} bind:value={textFilter} />
      <div class="texts">
        {#each textRows as [k, v] (k)}
          <label class="tkey" for={`t-${k}`}><code>{k}</code></label>
          <input id={`t-${k}`} value={v} disabled={readonly} onchange={(e) => setTextValue(k, e.currentTarget.value)} />
        {/each}
      </div>
      {#if !readonly}
        <div class="row">
          <input placeholder={t('g_key')} aria-label={t('g_key')} bind:value={newKey} style="max-width: 260px" />
          <input class="grow" placeholder={t('g_text')} aria-label={t('g_text')} bind:value={newValue} />
          <button class="btn small" disabled={!newKey.trim() || !newValue.trim()} onclick={addText}>{t('add')}</button>
        </div>
      {/if}
    </section>
  {:else if section}
    {@const s = section}
    {#if js[s.file] === undefined}
      <p class="muted">{t('g_no_file').replace('{f}', s.file)}</p>
    {:else if js[s.file] === null}
      <p class="warn">{t('g_syntax').replace('{f}', s.file)}</p>
    {:else}
      {#each s.groups as g (g.title)}
        <section class="stack group">
          {#if g.title}<h3>{g.title}</h3>{/if}
          {#each g.items as item, i (i)}
            {@const id = String(item.id ?? '')}
            {@const key = `${s.id}:${g.title}:${id}`}
            <details class="card item" open={openItem === key} ontoggle={(e) => e.currentTarget.open && (openItem = key)}>
              <summary><code>{id}</code> <span class="muted">{titleOf(item)}</span></summary>
              <div class="fields">
                {#each s.specs as sp (sp.key.join('.'))}
                  {@const fid = `${key}:${sp.key.join('.')}`}
                  <label for={fid}>{sp.label}</label>
                  {#if sp.kind === 'id'}
                    <input id={fid} value={id} disabled={readonly} onchange={(e) => rename(s, g, i, id, e.currentTarget.value)} />
                  {:else if sp.kind === 'bool'}
                    <input id={fid} type="checkbox" class="check" checked={at(item, sp.key) === true} disabled={readonly} onchange={(e) => change(s, g, i, sp, e.currentTarget.checked)} />
                  {:else if sp.kind === 'number'}
                    <input id={fid} type="number" value={shown(item, sp)} placeholder={sp.placeholder ?? ''} disabled={readonly} onchange={(e) => change(s, g, i, sp, e.currentTarget.value)} />
                  {:else if sp.kind === 'select'}
                    {@const opts = sp.options?.(g) ?? []}
                    {@const cur = shown(item, sp)}
                    <select id={fid} value={cur} disabled={readonly} onchange={(e) => change(s, g, i, sp, e.currentTarget.value)}>
                      <option value=""></option>
                      {#if cur && !opts.includes(cur)}<option value={cur}>{cur}</option>{/if}
                      {#each opts as o (o)}<option value={o}>{o}</option>{/each}
                    </select>
                  {:else if sp.kind === 'yaml'}
                    <textarea id={fid} class="code" rows="2" spellcheck="false" placeholder={sp.placeholder ?? ''} value={shown(item, sp)} disabled={readonly} onchange={(e) => change(s, g, i, sp, e.currentTarget.value)}></textarea>
                  {:else if sp.kind === 'label'}
                    {@const k = shown(item, sp)}
                    <div class="row label">
                      <input id={fid} class="lkey" value={k} aria-label={`${sp.label} — ${t('g_key')}`} disabled={readonly} onchange={(e) => change(s, g, i, sp, e.currentTarget.value)} />
                      <input class="grow" value={texts[k] ?? ''} placeholder={t('g_text_missing')} aria-label={`${sp.label} — ${t('g_text')} (${locale})`} disabled={readonly || !k} onchange={(e) => setTextValue(k, e.currentTarget.value)} />
                    </div>
                  {:else}
                    <input id={fid} value={shown(item, sp)} disabled={readonly} onchange={(e) => change(s, g, i, sp, e.currentTarget.value)} />
                  {/if}
                {/each}
              </div>
              {#if !readonly}
                <div class="row end"><button class="btn small" style="color: var(--danger)" onclick={() => remove(s, g, i, id)}>{t('g_remove')}</button></div>
              {/if}
            </details>
          {/each}
          {#if !readonly}
            <div class="row">
              <input placeholder={t('g_new_id')} aria-label={`${t('g_new_id')} ${g.title}`} value={newIds[`${s.id}:${g.title}`] ?? ''} oninput={(e) => (newIds[`${s.id}:${g.title}`] = e.currentTarget.value)} style="max-width: 260px" />
              <button class="btn small" onclick={() => add(s, g)}>{t('add')}</button>
            </div>
          {/if}
        </section>
      {/each}
    {/if}
  {/if}
</div>

<style>
  .guided { gap: 12px; }
  .tabs { flex-wrap: wrap; gap: 6px; }
  .tab { border: 1px solid var(--border); background: var(--surface); color: var(--text); border-radius: 999px; padding: 6px 14px; font: inherit; font-size: 14px; cursor: pointer; }
  .tab.active { background: var(--primary-soft); color: var(--primary-strong); border-color: transparent; font-weight: 600; }
  .group { gap: 8px; }
  .group h3 { margin: 8px 0 0; }
  .item { padding: 10px 14px; }
  .item summary { cursor: pointer; display: flex; gap: 10px; align-items: baseline; }
  .fields { display: grid; grid-template-columns: 180px 1fr; gap: 8px 12px; align-items: center; margin-top: 12px; }
  .fields label { font-size: 14px; color: var(--muted); }
  .code { font-family: ui-monospace, 'SF Mono', Menlo, monospace; font-size: 13px; width: 100%; resize: vertical; }
  .check { justify-self: start; width: 20px; height: 20px; }
  .label { gap: 8px; flex-wrap: nowrap; }
  .lkey { max-width: 220px; font-family: ui-monospace, 'SF Mono', Menlo, monospace; font-size: 13px; }
  .end { justify-content: flex-end; margin-top: 8px; }
  .texts { display: grid; grid-template-columns: minmax(160px, 280px) 1fr; gap: 6px 12px; align-items: center; max-height: 60vh; overflow: auto; }
  .tkey { font-size: 13px; overflow-wrap: anywhere; }
  .warn { color: var(--warn); }
  @media (max-width: 700px) { .fields, .texts { grid-template-columns: 1fr; } }
</style>
