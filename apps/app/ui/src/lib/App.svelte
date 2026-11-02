<script lang="ts">
  import { onMount } from 'svelte';
  import type { AppStatus_Serialize as AppStatus, MenuItem } from './bindings';
  import { api, run, Failure } from './api';
  import { t } from './i18n';
  import { route, go } from './router.svelte';
  import { toasts, toast, fail } from './toast.svelte';
  import { isMobile, biometricCheck, applyInsets } from './mobile';
  import Splash from './components/Splash.svelte';
  import Icon from './components/Icon.svelte';
  import { grouped, primary } from './nav';
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
  let showMore = $state(false);
  let showApps = $state(false);
  let tick = $state(0);
  let seen = -1;
  const EDITING = ['form', 'config', 'program_editor', 'people', 'roles', 'settings', 'resource_detail', 'units'];

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
    showMore = false;
    go(m.view, m.params);
  }
  const isActive = (m: MenuItem) =>
    m.view === route.view &&
    Object.keys(m.params).length === Object.keys(route.params).length &&
    Object.entries(m.params).every(([k, v]) => route.params[k] === v);
  const params = $derived(route.params);
  const groups = $derived(grouped(menu));
  const bottom = $derived(primary(menu, 4));
  const rest = $derived(menu.filter((m) => !bottom.includes(m)));
</script>

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
          <button type="button" class="tool app" aria-haspopup="true" aria-expanded={showApps} aria-label={`${status.branding.app_name} · ${t('menu')}`} title={status.branding.app_name} onclick={() => (showApps = !showApps)}>
            <img src="/package/logo.png" alt="" onerror={(e) => e.currentTarget.replaceWith(Object.assign(document.createElement('span'), { textContent: status?.branding.app_name.slice(0, 1) ?? '' }))} />
          </button>
          {#if showApps}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <div class="backdrop" onclick={() => (showApps = false)}></div>
            <div class="dropdown" role="menu" tabindex="-1" onkeydown={(e) => e.key === 'Escape' && (showApps = false)}>
              <p class="app-name">{status.branding.app_name}</p>
              {#each groups.filter((g) => g.section !== 'me') as g (g.section)}
                <p class="group-title">{t(`nav_${g.section}` as never)}</p>
                {#each g.items as m (m.label)}
                  <a href={`#/${m.view}`} role="menuitem" class="item" aria-current={isActive(m) ? 'page' : undefined} onclick={(e) => { e.preventDefault(); showApps = false; navigate(m); }}>
                    <Icon name={m.view} /><span>{m.label}</span>
                  </a>
                {/each}
              {/each}
            </div>
          {/if}
        </div>
        <span class="sep" aria-hidden="true"></span>
        {#each groups.find((g) => g.section === 'me')?.items ?? [] as m (m.label)}
          <a href={`#/${m.view}`} class="tool" aria-current={isActive(m) ? 'page' : undefined} aria-label={m.label} title={m.label} onclick={(e) => { e.preventDefault(); showApps = false; navigate(m); }}>
            <Icon name={m.view} size={22} />
          </a>
        {/each}
      </nav>
      <a class="sync" href="#/sync_status" title={status.unsynced ? status.unsynced_label : t('synced')} onclick={(e) => { e.preventDefault(); go('sync_status'); }}>
        <span class="dot" style={`background: ${status.unsynced ? 'var(--warn)' : 'var(--ok)'}`}></span>
        <span class="sync-label">{status.unsynced ? status.unsynced_label : t('synced')}</span>
      </a>
    </header>
    <main class="content">
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
        {:else if route.view === 'settings'}<Settings hasPin={status.has_pin} biometric={status.biometric} onchange={refresh} />
        {:else}<Home me={status.me} onchange={refresh} />{/if}
      {/key}
    </main>
    <nav class="bottom" aria-label={t('menu')}>
      {#each bottom as m (m.label)}
        <a href={`#/${m.view}`} aria-current={isActive(m) ? 'page' : undefined} onclick={(e) => { e.preventDefault(); navigate(m); }}><Icon name={m.view} size={22} /><span>{m.label}</span></a>
      {/each}
      <button type="button" aria-expanded={showMore} onclick={() => (showMore = !showMore)}><Icon name="more" size={22} /><span>{t('more')}</span></button>
    </nav>
    {#if showMore}
      <div class="sheet" role="dialog" aria-label={t('menu')}>
        {#each rest as m (m.label)}
          <button type="button" class="item" aria-current={isActive(m) ? 'page' : undefined} onclick={() => navigate(m)}><Icon name={m.view} /><span>{m.label}</span></button>
        {/each}
      </div>
    {/if}
  </div>
{/if}

{#if printing.data}<PrintExport data={printing.data} />{/if}

<div class="toasts" aria-live="polite">
  {#each toasts as x (x.id)}<div class="toast" class:error={x.error}>{x.text}</div>{/each}
</div>

<style>
  .center { min-height: 100vh; display: flex; flex-direction: column; justify-content: center; padding: 32px 20px; margin: 0 auto; }
  .narrow { max-width: 460px; }
  .notice { max-height: 55vh; overflow: auto; font-size: 15px; }
  .shell { min-height: 100vh; }
  .toolbar { position: sticky; top: 0; z-index: 30; display: flex; align-items: center; justify-content: space-between; gap: 12px; height: 56px; padding: 0 16px; background: var(--bg); border-bottom: 1px solid var(--border); }
  .tools { display: flex; align-items: center; gap: 4px; }
  .tool { display: grid; place-items: center; width: 42px; height: 42px; border-radius: 10px; color: var(--muted); background: none; border: none; cursor: pointer; text-decoration: none; }
  .tool:hover { background: var(--primary-soft); color: var(--text); }
  .tool[aria-current='page'] { background: var(--primary-soft); color: var(--primary-strong); }
  .tool.app img { width: 28px; height: 28px; }
  .tool.app :global(span) { font-weight: 700; color: var(--primary); }
  .tool.app[aria-expanded='true'] { background: var(--primary-soft); }
  .sep { width: 1px; height: 24px; background: var(--border); margin: 0 6px; }
  .appmenu { position: relative; }
  .backdrop { position: fixed; inset: 0; z-index: 40; }
  .dropdown { position: absolute; top: 48px; left: 0; z-index: 41; width: 260px; max-height: calc(100vh - 72px); overflow-y: auto; padding: 8px; background: var(--bg); border: 1px solid var(--border); border-radius: 12px; box-shadow: 0 12px 32px rgb(0 0 0 / 0.14); display: flex; flex-direction: column; gap: 2px; }
  .app-name { margin: 6px 12px 4px; font-weight: 700; background: var(--brand-gradient); -webkit-background-clip: text; background-clip: text; color: transparent; width: fit-content; }
  .group-title { margin: 10px 12px 4px; font-size: 12px; font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; color: var(--muted); }
  .item { display: flex; align-items: center; gap: 12px; padding: 9px 12px; border-radius: 8px; color: var(--text); text-decoration: none; font-size: 15px; background: none; border: none; text-align: left; font: inherit; cursor: pointer; width: 100%; }
  .item :global(svg) { flex: none; color: var(--muted); }
  .item[aria-current='page'] { background: var(--primary-soft); color: var(--primary-strong); font-weight: 600; }
  .item[aria-current='page'] :global(svg) { color: var(--primary-strong); }
  .item:hover:not([aria-current='page']) { background: var(--primary-soft); }
  .sync { font-size: 13px; color: var(--muted); display: flex; align-items: center; gap: 8px; text-decoration: none; padding: 8px 10px; border-radius: 8px; }
  .sync:hover { background: var(--primary-soft); }
  .content { min-width: 0; padding: 40px 32px 96px; }
  .content :global(> *) { max-width: 820px; margin-left: auto; margin-right: auto; }
  .bottom { display: none; }
  .sheet { display: none; }
  .toasts { position: fixed; right: 16px; bottom: 16px; display: flex; flex-direction: column; gap: 8px; z-index: 50; }
  .toast { background: var(--text); color: var(--bg); padding: 12px 16px; border-radius: 10px; font-size: 15px; max-width: 360px; box-shadow: 0 4px 16px rgb(0 0 0 / 0.15); }
  .toast.error { background: var(--danger); color: var(--bg); }
  @media (max-width: 760px) {
    .toolbar { display: none; }
    .content { padding: 32px 20px 110px; }
    .bottom { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); position: fixed; left: 0; right: 0; bottom: 0; background: var(--bg); border-top: 1px solid var(--border); padding: 6px 4px calc(10px + var(--inset-bottom, env(safe-area-inset-bottom, 0px))); }
    .bottom { grid-template-columns: repeat(5, minmax(0, 1fr)); }
    .bottom a, .bottom button { min-height: 56px; display: flex; flex-direction: column; gap: 3px; align-items: center; justify-content: center; text-align: center; color: var(--muted); text-decoration: none; font: inherit; font-size: 11px; line-height: 1.2; background: none; border: none; padding: 4px 2px; }
    .bottom span { max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .bottom a[aria-current='page'] { color: var(--primary); font-weight: 600; }
    .sheet { display: flex; flex-direction: column; position: fixed; left: 0; right: 0; bottom: 70px; background: var(--bg); border-top: 1px solid var(--border); padding: 8px 12px; max-height: 60vh; overflow-y: auto; box-shadow: 0 -8px 24px rgb(0 0 0 / 0.1); }
    .toasts { bottom: 90px; left: 16px; }
  }
</style>
