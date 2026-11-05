<script lang="ts">
  // The rows of a summary: a heading for each section (first column, as in
  // the paper form), bold rows where the package says so.
  import type { Measure } from '../bindings';

  let { measures }: { measures: Measure[] } = $props();
  // Consecutive rows of the same section share one heading cell.
  const rows = $derived(
    measures.map((m, i) => {
      const first = i === 0 || measures[i - 1].section !== m.section;
      let span = 1;
      if (first) while (measures[i + span]?.section === m.section && i + span < measures.length) span++;
      return { m, first, span };
    })
  );
  const sections = $derived(measures.some((m) => m.section));
</script>

<table class="measures"><tbody>
  {#each rows as { m, first, span }, i (i)}
    <tr class:strong={m.strong} class:start={first}>
      {#if sections && first}<th scope="row" rowspan={span}>{m.section ?? ''}</th>{/if}
      <td>{m.label}</td>
      <td class="num">{m.display}</td>
    </tr>
  {/each}
</tbody></table>

<style>
  .measures { width: 100%; border-collapse: collapse; font-size: 15px; }
  .measures th, .measures td { padding: 8px 10px; border-bottom: 1px solid var(--border); text-align: left; vertical-align: middle; }
  .measures th { font-size: 13px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.02em; color: var(--text); width: 30%; }
  .measures .num { text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .strong td { font-weight: 700; }
  .measures tr:last-child td, .measures tr:last-child th { border-bottom: none; }
  @media (max-width: 640px) {
    .measures th { width: auto; font-size: 11px; }
  }
</style>
