<script lang="ts">
  // Printable page of a person's data (§14): the system print dialog saves
  // it as PDF.
  import type { PersonExport_Serialize as PersonExport } from '../bindings';
  import { t } from '../i18n';

  let { data }: { data: PersonExport } = $props();
</script>

<article class="print-area" aria-hidden="true">
  <h1>{t('export_title')}: {data.person.name}</h1>
  <p>{t('export_generated')} {data.generated_at}{data.erasure_requested ? ` · ${t('erasure_requested')}` : ''}</p>
  {#if data.person.attributes.length || data.person.unit}
    <h2>{t('export_person')}</h2>
    <table>
      <tbody>
        {#if data.person.unit}<tr><th>{t('unit')}</th><td>{data.person.unit}</td></tr>{/if}
        {#each data.person.attributes as a (a.label)}<tr><th>{a.label}</th><td>{a.value}</td></tr>{/each}
      </tbody>
    </table>
  {/if}
  {#if data.person.qualifications.length || data.person.roles.length}
    <h2>{t('export_grants')}</h2>
    <ul>
      {#each data.person.qualifications as g (g.grant_id)}<li>{g.label}{g.period ? ` (${g.period})` : ''}</li>{/each}
      {#each data.person.roles as g (g.grant_id)}<li>{g.label}</li>{/each}
    </ul>
  {/if}
  {#if data.documents.length}
    <h2>{t('export_documents')}</h2>
    {#each data.documents as d, i (i)}
      <h3>{d.form} · {d.period} · {d.state}</h3>
      <table>
        <tbody>{#each d.values as v (v.label)}<tr><th>{v.label}</th><td>{v.value}</td></tr>{/each}</tbody>
      </table>
      {#if d.history.length}<p class="small">{d.history.join(' → ')}</p>{/if}
    {/each}
  {/if}
  {#if data.assignments.length}
    <h2>{t('export_assignments')}</h2>
    <ul>{#each data.assignments as a, i (i)}<li>{a.date} · {a.program} · {a.slot}</li>{/each}</ul>
  {/if}
  {#if data.resources.length}
    <h2>{t('export_resources')}</h2>
    <ul>{#each data.resources as r, i (i)}<li>{r.resource}: {r.since}{r.until ? ` – ${r.until}` : ''}</li>{/each}</ul>
  {/if}
</article>

<style>
  .print-area { display: none; }
  :global(html.printing) .print-area { display: block; color: #000; background: #fff; font-size: 12pt; }
  table { border-collapse: collapse; width: 100%; margin: 4pt 0 10pt; }
  th, td { border: 1px solid #999; padding: 3pt 6pt; text-align: left; vertical-align: top; }
  th { width: 40%; font-weight: 600; }
  .small { font-size: 10pt; }
  @media print {
    :global(html.printing body *) { visibility: hidden; }
    :global(html.printing) .print-area, :global(html.printing) .print-area :global(*) { visibility: visible; }
    :global(html.printing) .print-area { position: absolute; left: 0; top: 0; width: 100%; }
  }
</style>
