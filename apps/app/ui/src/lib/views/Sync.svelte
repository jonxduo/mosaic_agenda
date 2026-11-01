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
  .qr { width: 240px; background: #fff; padding: 8px; border-radius: 8px; }
  .qr :global(svg) { width: 100%; height: auto; }
</style>
