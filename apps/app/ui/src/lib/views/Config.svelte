<script lang="ts">
  import { ask } from '../ask.svelte';
  // Configuration editor (§12.4): the package files of the active version,
  // checked while typing, simulated on a copy of the data, then published as
  // a new version. Desktop view.
  import type { ConfigFile, ConfigIssue, ConfigVersion_Serialize as ConfigVersion, ConfigSimulation_Serialize as ConfigSimulation } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail, toast } from '../toast.svelte';
  import ConfigGuided from './ConfigGuided.svelte';

  let { onchange }: { onchange: () => void } = $props();
  let current = $state<ConfigVersion | null>(null);
  let files = $state<ConfigFile[]>([]);
  let selected = $state('');
  let issues = $state<ConfigIssue[]>([]);
  let sim = $state<ConfigSimulation | null>(null);
  let busy = $state(false);
  let mode = $state<'guided' | 'yaml'>('guided');
  let timer: ReturnType<typeof setTimeout> | undefined;

  const original = (path: string) => current?.files.find((f) => f.path === path)?.text;
  const changed = $derived(files.filter((f) => f.text !== original(f.path)).map((f) => f.path));
  const errors = $derived(issues.filter((i) => i.severity === 'error'));
  const file = $derived(files.find((f) => f.path === selected));

  async function load() {
    try {
      current = await run(api.configFiles());
      files = current.files.map((f) => ({ ...f }));
      selected ||= files.find((f) => f.path === 'package.yaml')?.path ?? files[0]?.path ?? '';
      issues = [];
      sim = null;
    } catch (e) {
      fail(e);
    }
  }
  load();

  function edited(text: string, path = selected) {
    const f = files.find((x) => x.path === path);
    if (!f) return;
    f.text = text;
    sim = null;
    clearTimeout(timer);
    timer = setTimeout(check, 600);
  }

  async function check() {
    try {
      issues = await run(api.configCheck(files));
    } catch (e) {
      fail(e);
    }
  }

  async function simulate() {
    busy = true;
    try {
      sim = await run(api.configSimulate(files));
      issues = sim.issues;
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }

  async function publish() {
    if (!current || !(await ask(t('config_confirm').replace('{n}', String(current.version + 1)), t('config_publish')))) return;
    busy = true;
    try {
      const v = await run(api.configPublish(files));
      toast(t('config_published').replace('{n}', String(v)));
      await load();
      onchange();
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }

  async function loadAppVersion() {
    try {
      const app = await run(api.configAppFiles());
      files = app.map((f) => ({ ...f }));
      sim = null;
      await check();
    } catch (e) {
      fail(e);
    }
  }

  function discard() {
    if (current) files = current.files.map((f) => ({ ...f }));
    issues = [];
    sim = null;
  }

  const count = (path: string) => issues.filter((i) => i.file === path && i.severity === 'error').length;
</script>

{#if current}
  <div class="stack page">
    <header class="spread">
      <div class="stack tight">
        <h1>{t('config')}</h1>
        <p class="muted">{t('config_version')} {current.version} · {t('config_package')} {current.package_version}</p>
      </div>
      {#if current.can_publish}
        <div class="row">
          <button class="btn" disabled={busy || !changed.length} onclick={discard}>{t('config_discard')}</button>
          <button class="btn" disabled={busy || !changed.length} onclick={simulate}>{t('config_simulate')}</button>
          <button class="btn primary" disabled={busy || !sim || errors.length > 0} onclick={publish}>{t('config_publish')} {current.version + 1}</button>
        </div>
      {/if}
    </header>

    {#if current.app_update}
      <div class="card row notice-update">
        <span class="grow">{t('config_app_update').replace('{v}', current.app_update)}</span>
        <button class="btn small" onclick={loadAppVersion}>{t('config_load_app')}</button>
      </div>
    {/if}

    <div class="row modes" role="radiogroup" aria-label={t('g_mode')}>
      <button class="btn small" role="radio" aria-checked={mode === 'guided'} class:primary={mode === 'guided'} onclick={() => (mode = 'guided')}>{t('g_guided')}</button>
      <button class="btn small" role="radio" aria-checked={mode === 'yaml'} class:primary={mode === 'yaml'} onclick={() => (mode = 'yaml')}>YAML</button>
      {#if changed.length}<span class="muted">{t('config_modified')}: {changed.join(', ')}</span>{/if}
    </div>

    {#if mode === 'guided'}
      <ConfigGuided {files} {original} readonly={!current.can_publish} onedit={(path, text) => edited(text, path)} />
      {#if issues.length}
        <section class="card stack issues" aria-live="polite">
          {#each issues as i, n (n)}
            <button class="issue" onclick={() => { selected = i.file; mode = 'yaml'; }}>
              <span class="badge" class:err={i.severity === 'error'}>{i.severity === 'error' ? t('config_error') : t('config_warning')}</span>
              <span class="grow"><strong>{i.file}</strong>{i.path ? ` · ${i.path}` : ''} — {i.message}</span>
            </button>
          {/each}
        </section>
      {:else if changed.length}
        <p class="muted">{t('config_valid')}</p>
      {/if}
    {:else}
    <div class="editor">
      <nav class="files" aria-label={t('config_files')}>
        {#each files as f (f.path)}
          <button class="file" class:active={f.path === selected} onclick={() => (selected = f.path)}>
            <span class="grow">{f.path}</span>
            {#if changed.includes(f.path)}<span class="dot" title={t('config_modified')}></span>{/if}
            {#if count(f.path)}<span class="badge err">{count(f.path)}</span>{/if}
          </button>
        {/each}
      </nav>
      <div class="stack main">
        {#if file}
          <textarea
            class="code"
            spellcheck="false"
            aria-label={file.path}
            readonly={!current.can_publish}
            value={file.text}
            oninput={(e) => edited(e.currentTarget.value)}
          ></textarea>
        {/if}
        {#if issues.length}
          <section class="card stack issues" aria-live="polite">
            {#each issues as i, n (n)}
              <button class="issue" onclick={() => (selected = i.file)}>
                <span class="badge" class:err={i.severity === 'error'}>{i.severity === 'error' ? t('config_error') : t('config_warning')}</span>
                <span class="grow"><strong>{i.file}</strong>{i.path ? ` · ${i.path}` : ''} — {i.message}</span>
              </button>
            {/each}
          </section>
        {:else if changed.length}
          <p class="muted">{t('config_valid')}</p>
        {/if}
      </div>
    </div>

    {/if}

    {#if sim && !errors.length}
      <section class="card stack">
        <h2>{t('config_simulation')}</h2>
        {#if sim.changes.length}
          <ul>
            {#each sim.changes as c (c.kind + c.id)}
              <li><span class="badge">{t(`config_${c.what}` as never)}</span> {c.kind} <code>{c.id}</code>{c.renamed_to ? ` → ${c.renamed_to}` : ''}</li>
            {/each}
          </ul>
        {:else}<p class="muted">{t('config_no_changes')}</p>{/if}
        {#each sim.audiences as a (a.audience)}
          <p><strong>{a.audience}</strong>: {#if a.added.length}+ {a.added.join(', ')} {/if}{#if a.removed.length}− {a.removed.join(', ')}{/if}</p>
        {/each}
        {#each sim.new_alerts as a (a)}<p class="warn">⚠ {a}</p>{/each}
        {#each sim.gone_alerts as a (a)}<p class="ok">✓ {a}</p>{/each}
        <p class="muted">{t('config_open_docs').replace('{n}', String(sim.open_documents))}</p>
      </section>
    {/if}
  </div>
{/if}

<style>
  .page { gap: 16px; }
  .tight { gap: 4px; }
  .editor { display: grid; grid-template-columns: 220px 1fr; gap: 16px; align-items: start; }
  .files { display: flex; flex-direction: column; gap: 2px; position: sticky; top: 16px; }
  .file { display: flex; align-items: center; gap: 6px; text-align: left; padding: 8px 10px; border-radius: 8px; border: none; background: none; font: inherit; font-size: 14px; color: var(--text); cursor: pointer; }
  .file.active { background: var(--primary-soft); color: var(--primary-strong); font-weight: 600; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--primary); }
  .main { min-width: 0; gap: 12px; }
  .code { width: 100%; min-height: 60vh; font-family: ui-monospace, 'SF Mono', Menlo, monospace; font-size: 13px; line-height: 1.5; tab-size: 2; white-space: pre; padding: 12px; border: 1px solid var(--border); border-radius: 10px; background: var(--surface); color: var(--text); resize: vertical; }
  .issues { max-height: 30vh; overflow: auto; gap: 6px; }
  .issue { display: flex; gap: 8px; align-items: baseline; text-align: left; border: none; background: none; font: inherit; font-size: 14px; color: var(--text); cursor: pointer; padding: 2px 0; }
  .badge.err { background: var(--danger); color: var(--bg); }
  .warn { color: var(--warn); margin: 0; }
  .ok { color: var(--ok); margin: 0; }
  .notice-update { background: var(--primary-soft); }
  .modes { gap: 8px; align-items: center; }
  ul { margin: 0; padding-left: 20px; }
  @media (max-width: 900px) { .editor { grid-template-columns: 1fr; } .files { position: static; flex-direction: row; flex-wrap: wrap; } }
</style>
