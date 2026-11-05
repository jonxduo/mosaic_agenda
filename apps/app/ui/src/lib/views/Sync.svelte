<script lang="ts">
  import type { SyncStatus_Serialize as Status, Pairing, Me, Invitation } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail, toast } from '../toast.svelte';

  let { me }: { me: Me | null } = $props();
  let s = $state<Status | null>(null);
  let pairings = $state<Pairing[]>([]);
  let invite = $state<Invitation | null>(null);
  let busy = $state(false);

  async function load() {
    s = await run(api.syncStatus()).catch((e) => { fail(e); return null; });
    pairings = await run(api.pairings()).catch(() => []);
  }
  load();
  const timer = setInterval(() => run(api.pairings()).then((p) => (pairings = p)).catch(() => {}), 2000);
  $effect(() => () => clearInterval(timer));

  async function syncNow() {
    busy = true;
    try {
      s = await run(api.syncNow());
      toast(t('synced'));
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }
  async function decide(code: string, accept: boolean) {
    try {
      await run(api.confirmPairing(code, accept));
      toast(t('saved'));
      await load();
    } catch (e) {
      fail(e);
    }
  }
  // Blind backup node (§10.3): the key printed by the server, set by a root
  // administrator; the organization's id goes in the server's configuration.
  let nodeKey = $state('');
  async function setNode(node: string | null) {
    try {
      await run(api.setBackupNode(node));
      nodeKey = '';
      toast(t('saved'));
      await load();
    } catch (e) {
      fail(e);
    }
  }
  async function inviteMine() {
    if (!me) return;
    invite = await run(api.inviteDevice(me.person_id)).catch((e) => { fail(e); return null; });
  }
</script>

<div class="stack page">
  {#if pairings.length}
    <section class="card stack">
      <h3>{t('pairing_requests')}</h3>
      {#each pairings as p (p.code)}
        <div class="list-row">
          <span class="grow">{p.person}</span><strong class="code">{p.code}</strong>
          <button class="btn small" onclick={() => decide(p.code, false)}>{t('pairing_refuse')}</button>
          <button class="btn small primary" onclick={() => decide(p.code, true)}>{t('pairing_confirm')}</button>
        </div>
      {/each}
    </section>
  {/if}
  {#if s}
    <section class="card">
      <table><tbody>
        <tr><td class="muted">{t('network')}</td><td>{s.online ? s.relay : '—'}</td></tr>
        <tr><td class="muted">{t('events')}</td><td>{s.events}</td></tr>
        <tr><td class="muted">{t('outbox')}</td><td>{s.outbox}</td></tr>
        <tr><td class="muted">{t('last_sync')}</td><td>{s.last_sync ?? '—'}</td></tr>
      </tbody></table>
    </section>
    {#if s.can_set_backup_node || s.backup_node}
      <section class="card stack">
        <h3>{t('backup_node')}</h3>
        <p class="muted small">{t('backup_node_hint')}</p>
        <table><tbody>
          <tr><td class="muted">{t('backup_node_key')}</td><td class="key">{s.backup_node ?? t('backup_node_none')}</td></tr>
          {#if s.can_set_backup_node && s.org_id}
            <tr><td class="muted">{t('org_id')}</td><td class="key">{s.org_id}</td></tr>
          {/if}
        </tbody></table>
        {#if s.can_set_backup_node}
          <form class="row" onsubmit={(e) => { e.preventDefault(); if (nodeKey.trim()) setNode(nodeKey.trim()); }}>
            <input class="grow" aria-label={t('backup_node_key')} placeholder={t('backup_node_placeholder')} bind:value={nodeKey} spellcheck="false" autocomplete="off" />
            <button class="btn" type="submit" disabled={!nodeKey.trim()}>{t('save_plain')}</button>
            {#if s.backup_node}<button class="btn" type="button" onclick={() => setNode(null)}>{t('backup_node_remove')}</button>{/if}
          </form>
        {/if}
      </section>
    {/if}
    <div class="row">
      <button class="btn primary" disabled={busy} onclick={syncNow}>{t('sync_now')}</button>
      {#if me}<button class="btn" onclick={inviteMine}>{t('invite')}</button>{/if}
    </div>
    {#if invite}
      <div class="stack">
        <p class="muted">{t('invite_hint')} {invite.expires_at}</p>
        <div class="qr">{@html invite.qr_svg}</div>
        <textarea readonly value={invite.ticket} aria-label={t('ticket')}></textarea>
      </div>
    {/if}
    <section class="stack">
      <h2>{t('peers')}</h2>
      {#each s.peers as p (p.device_id)}<div class="list-row"><span class="grow">{p.person}</span>{#if p.archive}<span class="badge">{t('archive')}</span>{/if}<span class="muted">{p.device_id.slice(0, 8)}</span></div>{/each}
    </section>
  {/if}
</div>

<style>
  .page { gap: 18px; }
  .code { letter-spacing: 0.12em; font-variant-numeric: tabular-nums; }
  .small { font-size: 13px; margin: 0; }
  /* Long hex keys: wrap anywhere, selectable for copying. */
  .key { font-family: ui-monospace, monospace; font-size: 13px; word-break: break-all; user-select: all; }
  .qr { width: 240px; background: #fff; padding: 8px; border-radius: 8px; }
  .qr :global(svg) { width: 100%; height: auto; }
</style>
