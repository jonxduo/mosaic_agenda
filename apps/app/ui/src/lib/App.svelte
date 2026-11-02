<script lang="ts">
  import { onMount } from 'svelte';
  import type { AppStatus_Serialize as AppStatus, MenuItem } from './bindings';
  import { api, run, Failure } from './api';
  import { t } from './i18n';
  import { route, go } from './router.svelte';
  import { toasts, toast, fail } from './toast.svelte';
  import { isMobile, biometricCheck, applyInsets } from './mobile';
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
  import Assignments from './views/Assignments.svelte';

  let status = $state<AppStatus | null>(null);
  let menu = $state<MenuItem[]>([]);
  let pin = $state('');
  let showMore = $state(false);
  let tick = $state(0);

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
      return;
    }
    try {
      if (status.state === 'ready') menu = await run(api.menu());
      if (status.branding.primary_color) document.documentElement.style.setProperty('--primary', status.branding.primary_color);
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
      tick++;
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
</script>

{#if !status}
  <main class="center"><p class="muted">…</p></main>
{:else if status.privacy_notice}
  <main class="center narrow stack">
    <h1>{t('privacy')}</h1>
    <div class="card notice">{status.privacy_notice}</div>
    <button class="btn primary" onclick={acceptPrivacy}>{t('privacy_accept')}</button>
  </main>
{:else if status.state === 'empty' || status.state === 'joining'}
  <Onboarding {status} onchange={refresh} />
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
    <nav class="side" aria-label={t('menu')}>
      <div class="brand">{status.branding.app_name}</div>
      {#each menu as m (m.label)}
        <a href={`#/${m.view}`} class="item" aria-current={isActive(m) ? 'page' : undefined} onclick={(e) => { e.preventDefault(); navigate(m); }}>{m.label}</a>
      {/each}
      <div class="grow"></div>
      <a class="sync" href="#/sync_status" onclick={(e) => { e.preventDefault(); go('sync_status'); }}>
        <span class="dot" style={`background: ${status.unsynced ? 'var(--warn)' : 'var(--ok)'}`}></span>
        {status.unsynced ? status.unsynced_label : t('synced')}
      </a>
    </nav>
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
        {:else if route.view === 'settings'}<Settings hasPin={status.has_pin} biometric={status.biometric} onchange={refresh} />
        {:else}<Home me={status.me} onchange={refresh} />{/if}
      {/key}
    </main>
    <nav class="bottom" aria-label={t('menu')}>
      {#each menu.slice(0, 3) as m (m.label)}
        <a href={`#/${m.view}`} aria-current={isActive(m) ? 'page' : undefined} onclick={(e) => { e.preventDefault(); navigate(m); }}>{m.label}</a>
      {/each}
      <button type="button" onclick={() => (showMore = !showMore)}>{t('more')}</button>
    </nav>
    {#if showMore}
      <div class="sheet" role="dialog" aria-label={t('menu')}>
        {#each menu.slice(3) as m (m.label)}
          <button type="button" class="item" onclick={() => navigate(m)}>{m.label}</button>
        {/each}
        <button type="button" class="item" onclick={() => { showMore = false; go('sync_status'); }}>{t('peers')}</button>
      </div>
    {/if}
  </div>
{/if}

<div class="toasts" aria-live="polite">
  {#each toasts as x (x.id)}<div class="toast" class:error={x.error}>{x.text}</div>{/each}
</div>

<style>
  .center { min-height: 100vh; display: flex; flex-direction: column; justify-content: center; padding: 32px 20px; margin: 0 auto; }
  .narrow { max-width: 460px; }
  .notice { white-space: pre-wrap; max-height: 55vh; overflow: auto; font-size: 15px; }
  .shell { display: flex; min-height: 100vh; }
  .side { flex: 0 0 240px; padding: 28px 16px; background: var(--surface); border-right: 1px solid var(--border); display: flex; flex-direction: column; gap: 4px; position: sticky; top: 0; height: 100vh; overflow-y: auto; }
  .brand { font-weight: 700; font-size: 17px; padding: 0 12px 20px; }
  .item { display: block; padding: 12px; border-radius: 10px; color: var(--text); text-decoration: none; font-size: 15px; background: none; border: none; text-align: left; font: inherit; cursor: pointer; width: 100%; }
  .item[aria-current='page'] { background: var(--primary-soft); color: var(--primary-strong); font-weight: 600; }
  .sync { padding: 12px; font-size: 13px; color: var(--muted); display: flex; align-items: center; gap: 8px; text-decoration: none; }
  .content { flex: 1 1 auto; min-width: 0; padding: 48px 48px 96px; }
  .content :global(> *) { max-width: 780px; }
  .bottom { display: none; }
  .sheet { display: none; }
  .toasts { position: fixed; right: 16px; bottom: 16px; display: flex; flex-direction: column; gap: 8px; z-index: 50; }
  .toast { background: var(--text); color: var(--bg); padding: 12px 16px; border-radius: 10px; font-size: 15px; max-width: 360px; box-shadow: 0 4px 16px rgb(0 0 0 / 0.15); }
  .toast.error { background: var(--danger); color: #fff; }
  @media (max-width: 760px) {
    .side { display: none; }
    .content { padding: 32px 20px 110px; }
    .bottom { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); position: fixed; left: 0; right: 0; bottom: 0; background: var(--bg); border-top: 1px solid var(--border); padding: 6px 4px calc(10px + var(--inset-bottom, env(safe-area-inset-bottom, 0px))); }
    .bottom a, .bottom button { min-height: 52px; display: flex; align-items: center; justify-content: center; text-align: center; color: var(--muted); text-decoration: none; font: inherit; font-size: 12px; background: none; border: none; }
    .bottom a[aria-current='page'] { color: var(--primary); font-weight: 600; }
    .sheet { display: flex; flex-direction: column; position: fixed; left: 0; right: 0; bottom: 70px; background: var(--bg); border-top: 1px solid var(--border); padding: 8px 12px; max-height: 60vh; overflow-y: auto; box-shadow: 0 -8px 24px rgb(0 0 0 / 0.1); }
    .toasts { bottom: 90px; left: 16px; }
  }
</style>
