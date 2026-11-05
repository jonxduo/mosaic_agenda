<script lang="ts">
  // "Il mio profilo": the data a person changes about themselves (the
  // package says which); the address is suggested from OpenStreetMap.
  import type { Profile_Serialize as Profile } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import FieldInput from '../components/FieldInput.svelte';
  import { fail, toast } from '../toast.svelte';

  let pr = $state<Profile | null>(null);
  let given = $state('');
  let family = $state('');
  let values = $state<Record<string, unknown>>({});
  let dates = $state<Record<string, string>>({});
  let saving = $state(false);
  let initial = '';
  const snapshot = () => JSON.stringify({ given, family, values, dates });
  const dirty = $derived(pr !== null && snapshot() !== initial);

  function fill(p: Profile) {
    pr = p;
    given = p.given_name ?? '';
    family = p.family_name ?? '';
    values = Object.fromEntries(p.fields.map((f) => [f.id, f.value ?? null]));
    dates = Object.fromEntries(p.dated.map((d) => [d.id, d.date ?? '']));
    initial = snapshot();
  }
  run(api.profile()).then(fill).catch(fail);

  // Address suggestions, asked a moment after typing stops.
  let suggestions = $state<string[]>([]);
  let timer: ReturnType<typeof setTimeout> | undefined;
  function typed(id: string, v: string) {
    values[id] = v;
    clearTimeout(timer);
    if (v.trim().length < 4) return (suggestions = []);
    timer = setTimeout(async () => {
      suggestions = await run(api.addressSearch(v)).catch(() => []);
    }, 700);
  }

  async function save() {
    if (!pr) return;
    saving = true;
    try {
      const attrs = Object.fromEntries(Object.entries(values).map(([k, v]) => [k, v ?? '']));
      fill(await run(api.saveProfile(pr.given_name === null ? null : given, pr.family_name === null ? null : family, attrs, dates)));
      toast(t('saved'));
    } catch (e) {
      fail(e);
    } finally {
      saving = false;
    }
  }
</script>

{#if pr}
  <div class="stack page">
    <header class="stack tight">
      <h1>{t('profile_title')}</h1>
      <p class="muted">{t('profile_hint')}</p>
    </header>
    <section class="card grid">
      {#if pr.given_name !== null}
        <div class="stack small"><label for="pf-given">{t('given_name')}</label><input id="pf-given" bind:value={given} autocomplete="given-name" /></div>
      {/if}
      {#if pr.family_name !== null}
        <div class="stack small"><label for="pf-family">{t('family_name')}</label><input id="pf-family" bind:value={family} autocomplete="family-name" /></div>
      {/if}
      {#each pr.dated as d (d.id)}
        <div class="stack small"><label for={`pf-${d.id}`}>{t('date_of').replace('{what}', d.label)}</label><input id={`pf-${d.id}`} type="date" bind:value={dates[d.id]} /></div>
      {/each}
      {#each pr.fields as f (f.id)}
        {#if f.id === pr.address}
          <div class="stack small address wide">
            <label for="pf-address">{f.label}</label>
            <input id="pf-address" autocomplete="off" value={(values[f.id] as string) ?? ''} oninput={(e) => typed(f.id, e.currentTarget.value)} />
            {#if suggestions.length}
              <ul class="suggest" role="listbox" aria-label={f.label}>
                {#each suggestions as s (s)}
                  <li><button type="button" role="option" aria-selected="false" onclick={() => { values[f.id] = s; suggestions = []; }}>{s}</button></li>
                {/each}
              </ul>
              <span class="muted osm">© OpenStreetMap</span>
            {/if}
          </div>
        {:else}
          <div class:wide={f.kind === 'long_text'}><FieldInput field={f} bind:value={values[f.id]} /></div>
        {/if}
      {/each}
    </section>
    <div class="row">
      <button class="btn primary" disabled={saving || !dirty} onclick={save}>{t('save_plain')}</button>
      {#if dirty}<span class="muted" role="status">{t('profile_unsaved')}</span>{/if}
    </div>
  </div>
{/if}

<style>
  .page { gap: 18px; }
  .tight { gap: 4px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 14px 18px; align-items: end; }
  .wide { grid-column: 1 / -1; }
  .address { position: relative; }
  .suggest { list-style: none; margin: 4px 0 0; padding: 4px; border: 1px solid var(--border); border-radius: 10px; background: var(--bg); box-shadow: 0 8px 24px rgb(0 0 0 / 0.12); display: flex; flex-direction: column; gap: 2px; }
  .suggest button { width: 100%; text-align: left; background: none; border: none; padding: 8px 10px; border-radius: 8px; font: inherit; font-size: 14px; color: var(--text); cursor: pointer; }
  .suggest button:hover { background: var(--primary-soft); }
  .osm { font-size: 11px; }
</style>
