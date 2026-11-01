<script lang="ts">
  import type { FormView_Serialize as FormView, Action } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import FieldInput from '../components/FieldInput.svelte';
  import { fail, toast } from '../toast.svelte';

  let { form, subject, period, onchange }: { form: string; subject?: string; period: string; onchange: () => void } = $props();
  let view = $state<FormView | null>(null);
  let values = $state<Record<string, unknown>>({});
  let note = $state('');
  let busy = $state(false);

  async function load() {
    try {
      view = await run(api.form(form, subject ?? null, period));
      values = Object.fromEntries(view.fields.map((f) => [f.id, f.value]));
    } catch (e) {
      fail(e);
    }
  }
  load();

  async function save(quiet = false) {
    if (!view) return;
    view = await run(api.saveDocument(form, view.subject_id, period, values));
    if (!quiet) toast(t('saved'));
  }

  async function act(a: Action) {
    if (!view) return;
    if (a.note === 'required' && !note.trim()) {
      toast(t('note_required'), true);
      return;
    }
    busy = true;
    try {
      if (view.editable) await save(true);
      if (!view.document_id) throw new Error('document');
      view = await run(api.stepDocument(view.document_id, a.id, note.trim() || null));
      values = Object.fromEntries(view.fields.map((f) => [f.id, f.value]));
      note = '';
      toast(view.state_label ?? t('saved'));
      onchange();
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if view}
  <form class="stack page" onsubmit={(e) => e.preventDefault()}>
    <header class="stack head">
      <a href="#/home" onclick={(e) => { e.preventDefault(); history.back(); }}>← {t('back')}</a>
      <h1>{view.title}</h1>
      {#if view.subject_name && subject}<p class="muted">{view.subject_name}</p>{/if}
      {#if view.subtitle}<p class="muted">{view.subtitle}</p>{/if}
      {#if view.state_label}<p><span class="badge" data-testid="state">{view.state_label}</span>{#if view.delivery} <span class="muted">· {view.delivery}</span>{/if}</p>{/if}
    </header>

    {#each view.fields as f (f.id)}
      <FieldInput field={f} bind:value={values[f.id]} disabled={!view.editable} />
    {/each}

    {#if view.actions.some((a) => a.note !== 'none')}
      <div class="stack small">
        <label for="note">{t('note')}</label>
        <textarea id="note" bind:value={note}></textarea>
      </div>
    {/if}

    <div class="stack actions">
      {#if view.readers && view.editable}<p class="muted">{view.readers}</p>{/if}
      <div class="row">
        {#if view.editable}<button type="button" class="btn" disabled={busy} onclick={() => save().catch(fail)}>{t('save')}</button>{/if}
        {#each view.actions as a (a.id)}
          <button type="button" class="btn" class:primary={a.primary} disabled={busy || a.disabled} onclick={() => act(a)}>{a.label}</button>
        {/each}
      </div>
    </div>

    {#if view.history.length}
      <section class="stack small">
        <h2>{t('history')}</h2>
        {#each view.history as h, i (i)}
          <div class="list-row"><span class="grow">{h.label} · {h.by}</span><span class="muted">{h.date}</span></div>
          {#if h.note}<p class="muted">{h.note}</p>{/if}
        {/each}
      </section>
    {/if}
  </form>
{/if}

<style>
  .page { gap: 28px; max-width: 560px; }
  .head { gap: 6px; }
  .small { gap: 8px; }
  .actions { border-top: 1px solid var(--border); padding-top: 16px; }
</style>
