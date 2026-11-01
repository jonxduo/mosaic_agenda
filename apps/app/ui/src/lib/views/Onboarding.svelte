<script lang="ts">
  import type { AppStatus_Serialize as AppStatus } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail } from '../toast.svelte';

  let { status, onchange }: { status: AppStatus; onchange: () => void } = $props();
  let orgName = $state('');
  let myName = $state('');
  let ticket = $state('');
  let code = $state<string | null>(null);
  let busy = $state(false);

  async function create() {
    busy = true;
    try {
      await run(api.createOrg(orgName.trim(), myName.trim()));
      onchange();
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }

  async function join() {
    busy = true;
    try {
      code = await run(api.joinOrg(ticket.trim()));
      const poll = setInterval(async () => {
        const s = await run(api.status()).catch(() => null);
        if (s?.state === 'ready') {
          clearInterval(poll);
          onchange();
        }
      }, 2000);
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }
</script>

<main class="wrap stack">
  <h1>{status.branding.app_name}</h1>
  {#if code || status.state === 'joining'}
    <div class="card stack">
      <p>{t('code_check')}</p>
      {#if code}<p class="code" data-testid="pair-code">{code}</p>{/if}
      <p class="muted">{t('waiting')}</p>
    </div>
  {:else}
    <section class="card stack" aria-labelledby="create">
      <h3 id="create">{t('create_org')}</h3>
      <label for="org">{t('org_name')}</label>
      <input id="org" bind:value={orgName} />
      <label for="me">{t('your_name')}</label>
      <input id="me" bind:value={myName} />
      <button class="btn primary" disabled={busy || !orgName.trim() || !myName.trim()} onclick={create}>{t('create')}</button>
    </section>
    <section class="card stack" aria-labelledby="join">
      <h3 id="join">{t('join_org')}</h3>
      <label for="ticket">{t('ticket')}</label>
      <textarea id="ticket" bind:value={ticket}></textarea>
      <button class="btn" disabled={busy || !ticket.trim()} onclick={join}>{t('join')}</button>
    </section>
  {/if}
</main>

<style>
  .wrap { max-width: 520px; margin: 0 auto; padding: 48px 20px; }
  .code { font-size: 40px; font-weight: 700; letter-spacing: 0.15em; font-variant-numeric: tabular-nums; }
</style>
