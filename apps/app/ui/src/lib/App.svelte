<script lang="ts">
  import { onMount } from 'svelte';
  import type { AppStatus_Serialize as AppStatus, MenuItem } from './bindings';
  import { api, run, Failure } from './api';
  import { t } from './i18n';
  import { route, go } from './router.svelte';
  import { toasts, toast, fail } from './toast.svelte';
  import { question, answer } from './ask.svelte';
  import { isMobile, biometricCheck, applyInsets } from './mobile';
  import Splash from './components/Splash.svelte';
  import Icon from './components/Icon.svelte';
  import { grouped, sectionOf } from './nav';
  import { markdown } from './markdown';
  import PrintExport from './components/PrintExport.svelte';
  import { printing } from './privacy.svelte';
  import Onboarding from './views/Onboarding.svelte';
  import Home from './views/Home.svelte';
  import Form from './views/Form.svelte';
  import MyDocuments from './views/MyDocuments.svelte';
  import WorkQueue from './views/WorkQueue.svelte';
  import Aggregate from './views/Aggregate.svelte';
  import Programs from './views/Programs.svelte';
  import Program from './views/Program.svelte';
  import Board from './views/Board.svelte';
  import Resources from './views/Resources.svelte';
  import Resource from './views/Resource.svelte';
  import MyResources from './views/MyResources.svelte';
  import People from './views/People.svelte';
  import Units from './views/Units.svelte';
  import Alerts from './views/Alerts.svelte';
  import Sync from './views/Sync.svelte';
  import Settings from './views/Settings.svelte';
  import Config from './views/Config.svelte';
  import Schedule from './views/Schedule.svelte';
  import QualificationMatrix from './views/QualificationMatrix.svelte';
  import Assignments from './views/Assignments.svelte';

  let status = $state<AppStatus | null>(null);
  // Start screen: the whole animation at the very first start (before the
  // privacy notice); later only when starting takes a while (phones open the
  // core for a few seconds; desktop is ready at once and shows nothing).
  let splash = $state<'wait' | 'on' | 'intro' | 'leaving' | 'off'>('wait');
  setTimeout(() => {
    if (!status) splash = 'on';
  }, 250);
  function endSplash() {
    if (splash === 'off' || splash === 'leaving') return;
    splash = 'leaving';
    setTimeout(() => (splash = 'off'), 400);
  }
  $effect(() => {
    if (!status) return;
    // Automated tests (navigator.webdriver) skip the intro.
    if (status.privacy_notice && !navigator.webdriver && (splash === 'wait' || splash === 'on')) {
      splash = 'intro';
    } else if (splash === 'wait') {
      splash = 'off';
    } else if (splash === 'on') {
      endSplash();
    }
  });
  let menu = $state<MenuItem[]>([]);
  let pin = $state('');
  let showApps = $state(false);
  let tick = $state(0);
  let seen = -1;
  const EDITING = ['form', 'config', 'schedule', 'qualification_matrix', 'program_editor', 'people', 'roles', 'settings', 'resource_detail', 'units'];

  applyInsets();

  async function refresh() {
    try {
      status = await run(api.status());
    } catch (e) {
      // Mobile: the core opens in the background right after launch.
      if (e instanceof Failure && e.kind === 'starting') {
        setTimeout(refresh, 300);
        return;
      }
      fail(e);
      // Never stay on an empty screen: try again shortly.
      if (!status) setTimeout(refresh, 2000);
      return;
    }
    try {
      if (status.state === 'ready') menu = await run(api.menu());
      if (status.branding.primary_color) document.documentElement.style.setProperty('--package-primary', status.branding.primary_color);
      document.title = status.branding.app_name;
    } catch (e) {
      fail(e);
    }
  }

  async function unlock() {
    try {
      if (await run(api.unlock(pin))) {
        pin = '';
        await refresh();
      } else toast(t('wrong_pin'), true);
    } catch (e) {
      fail(e);
      await refresh();
    }
  }

  async function unlockBiometric() {
    try {
      await biometricCheck(t('biometric_reason'));
      if (await run(api.unlockBiometric())) await refresh();
    } catch (e) {
      fail(e);
      await refresh();
    }
  }

  async function acceptPrivacy() {
    await run(api.acceptPrivacy());
    await refresh();
  }

  // Auto-lock after inactivity (§8.6) and periodic refresh of the sync indicator.
  let last = Date.now();
  onMount(() => {
    refresh();
    const mark = () => (last = Date.now());
    for (const ev of ['pointerdown', 'keydown']) window.addEventListener(ev, mark);
    const timer = setInterval(async () => {
      if (!status || status.state !== 'ready') return;
      const minutes = await run(api.settings()).then((s) => s.auto_lock_minutes).catch(() => 5);
      if (status.has_pin && Date.now() - last > minutes * 60_000) {
        await run(api.lock());
        await refresh();
        return;
      }
      const notes = await run(api.notifications()).catch(() => []);
      for (const n of notes) toast(n.text);
      const s = await run(api.status()).catch(() => null);
      if (s) status = s;
      // Reload the view only when new events arrived, and never where the
      // person may be typing (it would lose what they wrote).
      const events = await run(api.syncStatus()).then((x) => x.events).catch(() => seen);
      if (events !== seen && !EDITING.includes(route.view)) tick++;
      seen = events;
    }, 20_000);
    return () => clearInterval(timer);
  });

  function navigate(m: MenuItem) {
    go(m.view, m.params);
  }
  const isActive = (m: MenuItem) =>
    m.view === route.view &&
    Object.keys(m.params).length === Object.keys(route.params).length &&
    Object.entries(m.params).every(([k, v]) => route.params[k] === v);
  const params = $derived(route.params);
  const groups = $derived(grouped(menu));
  const mine = $derived(menu.filter((m) => sectionOf(m) === 'me').slice(0, 4));
  const work = $derived(menu.filter((m) => sectionOf(m) === 'work'));
  const admin = $derived(menu.filter((m) => sectionOf(m) === 'admin'));
  const settings = $derived(menu.find((m) => m.view === 'settings'));
  const inAdmin = $derived(admin.some((m) => m.view === route.view));
</script>

{#snippet appEntries()}
              {#each work as m (m.label)}
                <a href={`#/${m.view}`} role="menuitem" class="item" aria-current={isActive(m) ? 'page' : undefined} onclick={(e) => { e.preventDefault(); showApps = false; navigate(m); }}>
                  <Icon name={m.view} /><span>{m.label}</span>
                </a>
              {/each}
              {#if work.length && (admin.length || settings)}<hr />{/if}
              {#if admin.length}
                <a href={`#/${admin[0].view}`} role="menuitem" class="item" aria-current={inAdmin ? 'page' : undefined} onclick={(e) => { e.preventDefault(); showApps = false; navigate(admin[0]); }}>
                  <Icon name="admin" /><span>{t('nav_admin')}</span>
                </a>
              {/if}
              {#if settings}
                {@const st = settings}
                <a href={`#/${st.view}`} role="menuitem" class="item" aria-current={isActive(st) ? 'page' : undefined} onclick={(e) => { e.preventDefault(); showApps = false; navigate(st); }}>
                  <Icon name={st.view} /><span>{st.label}</span>
                </a>
              {/if}
{/snippet}

{#if splash === 'on' || splash === 'intro' || splash === 'leaving'}<Splash leaving={splash === 'leaving'} intro={splash === 'intro'} onend={endSplash} />{/if}
{#if !status}
  <main class="center"></main>
{:else if status.privacy_notice}
  <main class="center narrow stack">
    <h1>{t('privacy')}</h1>
    <div class="card notice prose">{@html markdown(status.privacy_notice)}</div>
    <button class="btn primary" onclick={acceptPrivacy}>{t('privacy_accept')}</button>
  </main>
{:else if status.state === 'empty' || status.state === 'joining'}
  <Onboarding {status} onchange={refresh} />
{:else if status.state === 'restart'}
  <main class="center narrow stack">
    <h1>{t('restart_title')}</h1>
    <p>{t('restart_text')}</p>
  </main>
{:else if status.state === 'wiped'}
  <main class="center narrow stack">
    <h1>{t('wiped_title')}</h1>
    <p>{t('wiped_text')}</p>
  </main>
{:else if status.state === 'locked'}
  <main class="center narrow stack">
    <h1>{t('locked')}</h1>
    <label for="pin">{t('pin')}</label>
    <input id="pin" type="password" inputmode="numeric" autocomplete="off" bind:value={pin} onkeydown={(e) => e.key === 'Enter' && unlock()} />
    <button class="btn primary" onclick={unlock}>{t('unlock')}</button>
    {#if isMobile && status.biometric}<button class="btn" onclick={unlockBiometric}>{t('biometric_unlock')}</button>{/if}
  </main>
{:else}
  <div class="shell">
    <header class="toolbar">
      <nav class="tools" aria-label={t('menu')}>
        <div class="appmenu">
          <button type="button" class="tool app" aria-haspopup="true" aria-expanded={showApps} aria-label={`${status.branding.app_name} · ${t('menu')}`} data-tip={showApps ? null : t('menu')} onclick={() => (showApps = !showApps)}>
            <img src="/package/logo.png" alt="" onerror={(e) => e.currentTarget.replaceWith(Object.assign(document.createElement('span'), { textContent: status?.branding.app_name.slice(0, 1) ?? '' }))} />
          </button>
          {#if showApps}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <div class="backdrop" onclick={() => (showApps = false)}></div>
            <div class="dropdown" role="menu" tabindex="-1" onkeydown={(e) => e.key === 'Escape' && (showApps = false)}>
              {@render appEntries()}
            </div>
          {/if}
        </div>
        <span class="sep" aria-hidden="true"></span>
        {#each groups.find((g) => g.section === 'me')?.items ?? [] as m (m.label)}
          <a href={`#/${m.view}`} class="tool" aria-current={isActive(m) ? 'page' : undefined} aria-label={m.label} data-tip={m.label} onclick={(e) => { e.preventDefault(); showApps = false; navigate(m); }}>
            <Icon name={m.view} size={22} />
          </a>
        {/each}
      </nav>
      <span class="sep" aria-hidden="true"></span>
      <a class="sync tool" href="#/sync_status" aria-current={route.view === 'sync_status' ? 'page' : undefined} aria-label={status.unsynced ? status.unsynced_label : t('synced')} data-tip={status.unsynced ? status.unsynced_label : t('synced')} onclick={(e) => { e.preventDefault(); showApps = false; go('sync_status'); }}>
        <Icon name="sync_status" size={22} />
        <span class="dot" style={`background: ${status.unsynced ? 'var(--warn)' : 'var(--ok)'}`}></span>
      </a>
    </header>
    <main class="content">
      {#if inAdmin && admin.length > 1}
        <nav class="subnav" aria-label={t('nav_admin')}>
          {#each admin as m (m.label)}
            <a href={`#/${m.view}`} aria-current={isActive(m) ? 'page' : undefined} onclick={(e) => { e.preventDefault(); navigate(m); }}><Icon name={m.view} size={18} /><span>{m.label}</span></a>
          {/each}
        </nav>
      {/if}
      {#key `${route.view}?${JSON.stringify(route.params)}:${tick}`}
        {#if route.view === 'home'}<Home me={status.me} onchange={refresh} />
        {:else if route.view === 'form'}<Form form={params.form} subject={params.subject} period={params.period} onchange={refresh} />
        {:else if route.view === 'my_documents'}<MyDocuments form={params.form} />
        {:else if route.view === 'work_queue' || route.view === 'batch'}<WorkQueue workflow={params.workflow} unitType={params.unit_type} onchange={refresh} />
        {:else if route.view === 'aggregate'}<Aggregate aggregate={params.aggregate} period={params.period} />
        {:else if route.view === 'program_list'}<Programs types={params.types} />
        {:else if route.view === 'program_editor'}<Program program={params.program} />
        {:else if route.view === 'board'}<Board />
        {:else if route.view === 'resource_list'}<Resources resourceType={params.resource_type} />
        {:else if route.view === 'resource_detail'}<Resource resource={params.resource} />
        {:else if route.view === 'my_resources'}<MyResources resourceType={params.resource_type} />
        {:else if route.view === 'my_assignments'}<Assignments />
        {:else if route.view === 'people' || route.view === 'roles'}<People me={status.me} />
        {:else if route.view === 'units'}<Units />
        {:else if route.view === 'admin_alerts'}<Alerts />
        {:else if route.view === 'sync_status'}<Sync me={status.me} />
        {:else if route.view === 'config'}<Config onchange={refresh} />
        {:else if route.view === 'schedule'}<Schedule onchange={refresh} />
        {:else if route.view === 'qualification_matrix'}{#key route.params.qualifications}<QualificationMatrix qualifications={route.params.qualifications} />{/key}
        {:else if route.view === 'settings'}<Settings hasPin={status.has_pin} biometric={status.biometric} onchange={refresh} />
        {:else}<Home me={status.me} onchange={refresh} />{/if}
      {/key}
    </main>
    <nav class="bottom" aria-label={t('menu')}>
      {#each mine as m (m.label)}
        <a href={`#/${m.view}`} aria-current={isActive(m) ? 'page' : undefined} onclick={(e) => { e.preventDefault(); showApps = false; navigate(m); }}><Icon name={m.view} size={22} /><span class="label">{m.label}</span></a>
      {/each}
      <button type="button" class="app" aria-haspopup="true" aria-expanded={showApps} aria-label={`${status.branding.app_name} · ${t('menu')}`} onclick={() => (showApps = !showApps)}>
        <img src="/package/logo.png" alt="" />
        <span class="label" aria-hidden="true">{t('menu')}</span>
        {#if status.unsynced}<span class="dot" style="background: var(--warn)"></span>{/if}
      </button>
    </nav>
    {#if showApps}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="backdrop sheet-backdrop" onclick={() => (showApps = false)}></div>
      <div class="sheet" role="menu" tabindex="-1" onkeydown={(e) => e.key === 'Escape' && (showApps = false)}>
        {@render appEntries()}
        <!-- Phone: the sync state is the last entry of the menu. -->
        <hr />
        <a href="#/sync_status" role="menuitem" class="item sync" aria-current={route.view === 'sync_status' ? 'page' : undefined} onclick={(e) => { e.preventDefault(); showApps = false; go('sync_status'); }}>
          <Icon name="sync_status" /><span>{status.unsynced ? status.unsynced_label : t('synced')}</span>
          <span class="dot" style={`background: ${status.unsynced ? 'var(--warn)' : 'var(--ok)'}`}></span>
        </a>
      </div>
    {/if}
  </div>
{/if}

{#if printing.data}<PrintExport data={printing.data} />{/if}

{#if question.current}
  <div class="ask-backdrop" role="presentation" onclick={() => answer(false)}></div>
  <div class="ask card stack" role="alertdialog" aria-modal="true" aria-labelledby="ask-text" tabindex="-1"
    onkeydown={(e) => e.key === 'Escape' && answer(false)}>
    <p id="ask-text">{question.current.text}</p>
    <div class="row ask-actions">
      <button class="btn" onclick={() => answer(false)}>{t('cancel')}</button>
      <!-- svelte-ignore a11y_autofocus -->
      <button class="btn primary" class:danger={question.current.danger} autofocus onclick={() => answer(true)}>{question.current.ok}</button>
    </div>
  </div>
{/if}

<div class="toasts" aria-live="polite">
  {#each toasts as x (x.id)}<div class="toast" class:error={x.error}>{x.text}</div>{/each}
</div>

<style>
  .center { min-height: 100vh; display: flex; flex-direction: column; justify-content: center; padding: 32px 20px; margin: 0 auto; }
  .narrow { max-width: 460px; }
  .notice { max-height: 55vh; overflow: auto; font-size: 15px; }
  .shell { min-height: 100vh; }
  /* Floating toolbar: centred, rounded, a little below the top edge. */
  .toolbar { position: sticky; top: 12px; z-index: 30; display: flex; align-items: center; gap: 4px; width: fit-content; margin: 12px auto 0; height: 56px; padding: 0 7px; background: var(--bg); border: 1px solid var(--border); border-radius: 18px; box-shadow: 0 6px 20px rgb(60 45 20 / 0.08); }
  .tools { display: flex; align-items: center; gap: 4px; }
  .tool { display: grid; place-items: center; width: 42px; height: 42px; border-radius: 10px; color: var(--muted); background: none; border: none; cursor: pointer; text-decoration: none; }
  .tool:hover { background: var(--primary-soft); color: var(--text); }
  .tool[aria-current='page'] { background: var(--primary-soft); color: var(--primary-strong); }
  .tool.app img { width: 28px; height: 28px; }
  .tool.app :global(span) { font-weight: 700; color: var(--primary); }
  .tool.app[aria-expanded='true'] { background: var(--primary-soft); }
  /* Tooltip under the toolbar icons. */
  .tool[data-tip] { position: relative; }
  .tool[data-tip]::after { content: attr(data-tip); position: absolute; top: calc(100% + 10px); left: 50%; transform: translate(-50%, -4px); padding: 5px 10px; border-radius: 8px; background: var(--text); color: var(--bg); font-size: 13px; font-weight: 500; white-space: nowrap; pointer-events: none; opacity: 0; transition: opacity 0.12s ease 0.25s, transform 0.12s ease 0.25s; z-index: 50; }
  .tool[data-tip]:hover::after, .tool[data-tip]:focus-visible::after { opacity: 1; transform: translate(-50%, 0); }
  .sep { width: 1px; height: 24px; background: var(--border); margin: 0 6px; }
  .appmenu { position: relative; }
  .backdrop { position: fixed; inset: 0; z-index: 40; }
  .dropdown { position: absolute; top: 48px; left: 0; z-index: 41; width: 260px; max-height: calc(100vh - 72px); overflow-y: auto; padding: 8px; background: var(--bg); border: 1px solid var(--border); border-radius: 12px; box-shadow: 0 12px 32px rgb(0 0 0 / 0.14); display: flex; flex-direction: column; gap: 2px; }
  .item { display: flex; align-items: center; gap: 12px; padding: 9px 12px; border-radius: 8px; color: var(--text); text-decoration: none; font-size: 15px; background: none; border: none; text-align: left; font: inherit; cursor: pointer; width: 100%; }
  .item :global(svg) { flex: none; color: var(--muted); }
  .item[aria-current='page'] { background: var(--primary-soft); color: var(--primary-strong); font-weight: 600; }
  .item[aria-current='page'] :global(svg) { color: var(--primary-strong); }
  .item:hover:not([aria-current='page']) { background: var(--primary-soft); }
  .sync { position: relative; }
  .sync .dot { position: absolute; top: 9px; right: 8px; width: 9px; height: 9px; border: 2px solid var(--bg); box-sizing: content-box; }
  .dropdown hr { border: none; border-top: 1px solid var(--border); margin: 6px 4px; }
  .subnav { display: flex; flex-wrap: wrap; gap: 4px; padding: 4px; margin-bottom: 24px; background: var(--bg); border: 1px solid var(--border); border-radius: 14px; width: fit-content; }
  .subnav a { display: flex; align-items: center; gap: 8px; padding: 8px 14px; border-radius: 10px; color: var(--muted); text-decoration: none; font-size: 14px; font-weight: 500; }
  .subnav a:hover { background: var(--primary-soft); color: var(--text); }
  .subnav a[aria-current='page'] { background: var(--primary-soft); color: var(--primary-strong); font-weight: 600; }
  .content { min-width: 0; padding: 40px 32px 96px; }
  .content :global(> *) { max-width: 820px; margin-left: auto; margin-right: auto; }
  .bottom { display: none; }
  .sheet, .sheet-backdrop { display: none; }
  .ask-backdrop { position: fixed; inset: 0; background: rgb(0 0 0 / 0.35); z-index: 60; }
  .ask { position: fixed; z-index: 61; left: 50%; top: 50%; transform: translate(-50%, -50%); width: min(440px, calc(100vw - 32px)); box-shadow: 0 12px 40px rgb(0 0 0 / 0.2); }
  .ask-actions { justify-content: flex-end; }
  .ask .danger { background: var(--danger); border-color: var(--danger); }
  .toasts { position: fixed; right: 16px; bottom: 16px; display: flex; flex-direction: column; gap: 8px; z-index: 50; }
  .toast { background: var(--text); color: var(--bg); padding: 12px 16px; border-radius: 10px; font-size: 15px; max-width: 360px; box-shadow: 0 4px 16px rgb(0 0 0 / 0.15); }
  .toast.error { background: var(--danger); color: var(--bg); }
  @media (max-width: 760px) {
    .toolbar { display: none; }
    .content { padding: 32px 20px 120px; }
    /* Phone: the same toolbar, floating at the bottom, icons only. */
    .bottom { display: flex; justify-content: space-around; align-items: center; position: fixed; left: 12px; right: 12px; bottom: calc(10px + var(--inset-bottom, env(safe-area-inset-bottom, 0px))); z-index: 30; height: 72px; padding: 0 6px; background: var(--bg); border: 1px solid var(--border); border-radius: 20px; box-shadow: 0 6px 20px rgb(60 45 20 / 0.12); }
    .bottom a, .bottom button { position: relative; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 3px; flex: 1 1 0; min-width: 0; height: 62px; border-radius: 14px; color: var(--muted); text-decoration: none; background: none; border: none; padding: 0; cursor: pointer; }
    .bottom a[aria-current='page'], .bottom button[aria-expanded='true'] { background: var(--primary-soft); color: var(--primary-strong); }
    .bottom .app img { width: 24px; height: 24px; }
    .bottom .label { font-size: 11px; line-height: 1.15; max-width: 100%; padding: 0 2px; text-align: center; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
    .bottom .app .dot { position: absolute; top: 6px; right: calc(50% - 18px); width: 9px; height: 9px; border-radius: 50%; border: 2px solid var(--bg); box-sizing: content-box; }
    .sheet .sync .dot { position: static; margin-left: auto; width: 9px; height: 9px; border-radius: 50%; flex: none; }
    .sheet-backdrop { display: block; z-index: 29; }
    .sheet { display: flex; flex-direction: column; gap: 2px; position: fixed; left: 12px; right: 12px; bottom: calc(88px + var(--inset-bottom, env(safe-area-inset-bottom, 0px))); z-index: 31; background: var(--bg); border: 1px solid var(--border); border-radius: 16px; padding: 8px; max-height: 65vh; overflow-y: auto; box-shadow: 0 12px 32px rgb(0 0 0 / 0.16); }
    .sheet hr { border: none; border-top: 1px solid var(--border); margin: 6px 4px; }
    .subnav { flex-wrap: nowrap; overflow-x: auto; max-width: 100%; }
    .subnav a { white-space: nowrap; }
    .toasts { bottom: 90px; left: 16px; }
  }
</style>
