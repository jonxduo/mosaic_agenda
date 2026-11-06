<script lang="ts">
  // "Non posso" on an assignment: for a slot with requirements, who could
  // take it instead (if one was found already), or a plain "No".
  import type { AssignmentItem, Choice } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { ask } from '../ask.svelte';
  import { fail, toast } from '../toast.svelte';

  let { item, ondone }: { item: AssignmentItem | null; ondone: () => void } = $props();
  let choices = $state<Choice[] | null>(null);
  let chosen = $state('');
  let busy = $state(false);

  $effect(() => {
    const a = item;
    choices = null;
    chosen = '';
    if (!a) return;
    run(api.substitutes(a.program_id, a.program_type, a.occurrence, a.slot))
      .then(async (list) => {
        if (item !== a) return;
        if (list.length) {
          choices = list;
          return;
        }
        // Nothing required, nobody to suggest: a plain confirmation.
        if (await ask(t('cannot_confirm').replace('{what}', `${a.title} · ${a.subtitle}`), t('decline'), true)) await send(a, null);
        else ondone();
      })
      .catch((e) => {
        fail(e);
        ondone();
      });
  });

  async function send(a: AssignmentItem, substitute: string | null) {
    busy = true;
    try {
      await run(api.respond(a.program_id, a.program_type, a.occurrence, a.slot, a.index, false, null, null, substitute));
      toast(t('saved'));
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
      ondone();
    }
  }
</script>

{#if item && choices}
  {@const a = item}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="backdrop" onclick={ondone}></div>
  <div class="card stack modal" role="dialog" aria-modal="true" aria-labelledby="cannot-title" tabindex="-1" onkeydown={(e) => e.key === 'Escape' && ondone()}>
    <h3 id="cannot-title">{t('substitute_question')}</h3>
    <p class="muted">{a.title} · {a.subtitle}</p>
    <select aria-label={t('substitute_pick')} bind:value={chosen}>
      <option value="">{t('substitute_pick')}</option>
      {#each choices as c (c.id)}<option value={c.id}>{c.label}</option>{/each}
    </select>
    <div class="row actions">
      <button class="btn" onclick={ondone} disabled={busy}>{t('cancel')}</button>
      <button class="btn danger" onclick={() => send(a, null)} disabled={busy}>{t('no')}</button>
      <button class="btn primary" onclick={() => send(a, chosen)} disabled={busy || !chosen}>{t('substitute_send')}</button>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; background: rgb(0 0 0 / 0.35); z-index: 50; }
  .modal { position: fixed; z-index: 51; left: 50%; top: 50%; transform: translate(-50%, -50%); width: min(460px, calc(100vw - 32px)); box-shadow: 0 12px 40px rgb(0 0 0 / 0.2); }
  .modal h3 { margin: 0; }
  .modal p { margin: 0; }
  .actions { justify-content: flex-end; gap: 8px; flex-wrap: wrap; }
  .danger { background: var(--danger); color: #fff; border-color: var(--danger); }
</style>
