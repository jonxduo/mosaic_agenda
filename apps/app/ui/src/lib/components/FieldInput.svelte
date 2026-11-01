<script lang="ts">
  import type { Field_Serialize as Field } from '../bindings';
  import { t } from '../i18n';

  let { field, value = $bindable(), disabled = false }: { field: Field; value: unknown; disabled?: boolean } = $props();
  const id = `f-${field.id}`;

  function num(v: unknown): number {
    return typeof v === 'number' ? v : Number(v ?? 0) || 0;
  }
  function clamp(n: number): number {
    if (field.min != null && n < field.min) return field.min;
    if (field.max != null && n > field.max) return field.max;
    return n;
  }
  // Durations are minutes, edited as hours + minutes.
  let hours = $state(Math.floor(num(value) / 60));
  let minutes = $state(num(value) % 60);
  function setDuration() {
    value = clamp(Math.max(0, Math.round(hours) * 60 + Math.round(minutes)));
  }
</script>

{#if field.kind === 'boolean'}
  <fieldset class="bool" {disabled}>
    <legend>{field.label}</legend>
    <div class="two">
      <label class="choice" class:selected={value === true}><input type="radio" name={id} checked={value === true} onchange={() => (value = true)} />{t('yes')}</label>
      <label class="choice" class:selected={value === false}><input type="radio" name={id} checked={value === false} onchange={() => (value = false)} />{t('no')}</label>
    </div>
  </fieldset>
{:else if field.kind === 'integer' || field.kind === 'decimal'}
  <div class="stack small">
    <label for={id}>{field.label}</label>
    <div class="stepper">
      <button type="button" class="btn" aria-label="−" {disabled} onclick={() => (value = clamp(num(value) - 1))}>−</button>
      <input {id} type="number" inputmode="numeric" min={field.min ?? undefined} max={field.max ?? undefined} {disabled}
        value={value ?? ''} oninput={(e) => (value = e.currentTarget.value === '' ? null : clamp(Number(e.currentTarget.value)))} />
      <button type="button" class="btn" aria-label="+" {disabled} onclick={() => (value = clamp(num(value) + 1))}>+</button>
    </div>
  </div>
{:else if field.kind === 'duration'}
  <div class="stack small">
    <span class="lbl">{field.label}</span>
    <div class="row">
      <input id={id} type="number" min="0" aria-label={field.label} {disabled} bind:value={hours} oninput={setDuration} style="width: 110px" /><span class="muted">h</span>
      <input type="number" min="0" max="59" aria-label={`${field.label} min`} {disabled} bind:value={minutes} oninput={setDuration} style="width: 90px" /><span class="muted">min</span>
    </div>
  </div>
{:else if field.kind === 'long_text'}
  <div class="stack small">
    <label for={id}>{field.label}</label>
    <textarea {id} maxlength={field.max_length ?? undefined} {disabled} value={(value as string) ?? ''} oninput={(e) => (value = e.currentTarget.value)}></textarea>
  </div>
{:else if field.kind === 'choice'}
  <div class="stack small">
    <label for={id}>{field.label}</label>
    <select {id} {disabled} value={(value as string) ?? ''} onchange={(e) => (value = e.currentTarget.value || null)}>
      <option value=""></option>
      {#each field.options as o (o.id)}<option value={o.id}>{o.label}</option>{/each}
    </select>
  </div>
{:else if field.kind === 'date'}
  <div class="stack small">
    <label for={id}>{field.label}</label>
    <input {id} type="date" {disabled} value={(value as string) ?? ''} oninput={(e) => (value = e.currentTarget.value || null)} />
  </div>
{:else if field.kind === 'photos'}
  <p class="muted">{field.label}</p>
{:else}
  <div class="stack small">
    <label for={id}>{field.label}</label>
    <input {id} type="text" maxlength={field.max_length ?? undefined} {disabled} value={(value as string) ?? ''} oninput={(e) => (value = e.currentTarget.value)} />
  </div>
{/if}

<style>
  .small { gap: 8px; }
  .lbl { font-weight: 600; font-size: 15px; }
  fieldset { border: none; margin: 0; padding: 0; }
  legend { padding: 0; margin-bottom: 10px; font-size: 17px; font-weight: 600; }
  .two { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; }
  .stepper { display: grid; grid-template-columns: 56px minmax(0, 140px) 56px; gap: 10px; }
  .stepper input { text-align: center; font-size: 22px; font-weight: 600; height: 56px; }
  .stepper .btn { height: 56px; font-size: 22px; padding: 0; }
</style>
