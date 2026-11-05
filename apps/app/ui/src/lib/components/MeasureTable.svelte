<script lang="ts">
  // A summary as the package lays it out: plain rows (with section
  // headings), a table of cells (row × column, "–" where there is none),
  // notes, and boxes with a large number.
  import type { Measure } from '../bindings';

  let { measures }: { measures: Measure[] } = $props();

  const plain = $derived(measures.filter((m) => !m.row && !m.tile && !m.note));
  const cells = $derived(measures.filter((m) => m.row && m.column));
  const notes = $derived(measures.filter((m) => m.note));
  const tiles = $derived(measures.filter((m) => m.tile));
  const uniq = (xs: string[]) => xs.filter((x, i) => xs.indexOf(x) === i);
  const rowNames = $derived(uniq(cells.map((m) => m.row ?? '')));
  const colNames = $derived(uniq(cells.map((m) => m.column ?? '')));
  const cell = (r: string, c: string) => cells.find((m) => m.row === r && m.column === c);

  // Consecutive plain rows of the same section share one heading cell.
  const rows = $derived(
    plain.map((m, i) => {
      const first = i === 0 || plain[i - 1].section !== m.section;
      let span = 1;
      if (first) while (i + span < plain.length && plain[i + span].section === m.section) span++;
      return { m, first, span };
    })
  );
  const sections = $derived(plain.some((m) => m.section));
</script>

<div class="summary">
  {#if plain.length}
    <table class="measures"><tbody>
      {#each rows as { m, first, span }, i (i)}
        <tr class:strong={m.strong}>
          {#if sections && first}<th scope="row" rowspan={span}>{m.section ?? ''}</th>{/if}
          <td>{m.label}</td>
          <td class="num">{m.display}</td>
        </tr>
      {/each}
    </tbody></table>
  {/if}

  {#if cells.length}
    <div class="scroll">
      <table class="grid">
        <thead><tr><th></th>{#each colNames as c (c)}<th scope="col" class="num">{c}</th>{/each}</tr></thead>
        <tbody>
          {#each rowNames as r (r)}
            <tr>
              <th scope="row">{r}</th>
              {#each colNames as c (c)}
                {@const x = cell(r, c)}
                <td class="num" class:strong={x?.strong} title={x?.label}>{x ? x.display : '–'}</td>
              {/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  {#each notes as n (n.label)}
    <p class="note"><span>{n.label}</span><strong>{n.display}</strong></p>
  {/each}

  {#if tiles.length}
    <div class="tiles">
      {#each tiles as x (x.label)}
        <div class="tile">
          <strong class="big">{x.display}</strong>
          <span class="muted">{x.label}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .summary { display: flex; flex-direction: column; gap: 14px; }
  .measures, .grid { width: 100%; border-collapse: collapse; font-size: 15px; }
  .measures th, .measures td, .grid th, .grid td { padding: 8px 10px; border-bottom: 1px solid var(--border); text-align: left; vertical-align: middle; }
  .measures th { font-size: 13px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.02em; width: 30%; }
  .grid thead th { font-size: 13px; font-weight: 600; color: var(--muted); }
  .grid tbody th { font-weight: 600; white-space: nowrap; }
  .num { text-align: right !important; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .strong td, td.strong { font-weight: 700; }
  .measures tr:last-child td, .measures tr:last-child th, .grid tr:last-child td, .grid tr:last-child th { border-bottom: none; }
  .scroll { overflow-x: auto; }
  .note { display: flex; justify-content: space-between; gap: 12px; margin: 0; padding: 8px 12px; border-radius: 8px; background: var(--surface); font-size: 14px; color: var(--muted); }
  .note strong { color: var(--text); font-variant-numeric: tabular-nums; }
  .tiles { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 10px; }
  .tile { display: flex; flex-direction: column; gap: 4px; padding: 14px; border: 1px solid var(--border); border-radius: 12px; background: var(--bg); }
  .big { font-size: 26px; line-height: 1.1; font-variant-numeric: tabular-nums; }
  .tile .muted { font-size: 13px; line-height: 1.3; }
  @media (max-width: 640px) {
    .tiles { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .measures th { width: auto; font-size: 11px; }
  }
</style>
