<script lang="ts">
  import type { ProgramEditor_Serialize as Editor, SlotCell_Serialize as Cell, SlotChange, DetailChange, PartChange, ProgramGroup } from '../bindings';
  import { api, run } from '../api';
  import { t } from '../i18n';
  import { fail, toast } from '../toast.svelte';

  // One layout for every program: a row per part (the part and its details
  // on the left, who does it on the right, one column per room) and, beside,
  // the people who can take a part this week, the longest without one first.
  // `readonly`: the version on the board, as text, inside another page.
  // titled: read only inside a group of programs, under the program's name.
  // sent: read only, the version handed in for publication (not yet on the board).
  let { program, readonly = false, titled = false, sent = false }: { program: string; readonly?: boolean; titled?: boolean; sent?: boolean } = $props();
  let p = $state<Editor | null>(null);
  let external = $state('');
  let reading = $state(false);

  // Choices and details stay here until "Salva" (or "Invia", which saves first).
  let pending = $state<Record<string, SlotChange>>({});
  let details = $state<Record<string, DetailChange>>({});
  // Parts removed from or added to an event, until saved.
  let parts = $state<Record<string, PartChange>>({});
  let busy = $state(false);
  let readingWeek = $state<number | null>(null);
  const keyOf = (c: { occurrence: number; slot: string; index: number }) => `${c.occurrence}|${c.slot}|${c.index}`;
  const changed = $derived(Object.keys(pending).length + Object.keys(details).length + Object.keys(parts).length);
  const valueOf = (c: Cell) => pending[keyOf(c)]?.person ?? c.assignee ?? '';
  const text = (v: unknown) => (v === null || v === undefined ? '' : String(v));
  const detailOf = (c: Cell, id: string) => details[keyOf(c)]?.values[id] ?? text(c.details.find((f) => f.id === id)?.value);
  const isRow = (c: Cell) => c.row === c.slot && !c.column;
  const inRow = (top: Cell, column: string | null) =>
    p?.cells.filter((x) => x.occurrence === top.occurrence && x.per_event === top.per_event && x.row === top.slot && x.index === top.index && (x.column ?? null) === column) ?? [];
  const sectionOf = (id: string | null) => p?.sections.find((x) => x.id === id);
  const shownOf = (c: Cell) => parts[keyOf(c)]?.shown ?? c.shown;
  // A cell shown in another's row (an assistant) follows that row.
  const topOf = (c: Cell) =>
    p?.cells.find((x) => x.occurrence === c.occurrence && x.per_event === c.per_event && x.slot === c.row && x.index === c.index) ?? c;
  const visible = (c: Cell) => shownOf(topOf(c));
  // Read-only, a row shows only if someone or something is in it.
  const filled = (top: Cell) =>
    (p?.cells ?? []).some(
      (x) => x.occurrence === top.occurrence && x.per_event === top.per_event && x.row === top.slot && x.index === top.index &&
        (!!x.assignee || x.details.some((f) => text(f.value)))
    );
  const rowsOf = (occurrence: number, perEvent: boolean) =>
    p?.cells.filter((c) => c.occurrence === occurrence && c.per_event === perEvent && isRow(c) && shownOf(c) && (!readonly || filled(c))) ?? [];
  // Rows of a week that can be added: one entry per slot, its first hidden position.
  const addable = (g: ProgramGroup) => {
    const occs = occsOf(g);
    const seen = new Set<string>();
    return (p?.cells ?? []).filter((c) => {
      if (!occs.has(c.occurrence) || !isRow(c) || shownOf(c)) return false;
      const k = `${c.occurrence}|${c.slot}`;
      if (seen.has(k)) return false;
      seen.add(k);
      return true;
    });
  };
  function setShown(c: Cell, on: boolean) {
    const k = keyOf(c);
    if (on === c.shown) delete parts[k];
    else parts[k] = { occurrence: c.occurrence, slot: c.slot, index: c.index, shown: on };
    // A removed part keeps nobody.
    if (!on) for (const x of [c, ...(p?.cells ?? []).filter((y) => y !== c && topOf(y) === c)]) if (valueOf(x)) choose(x, '');
  }
  // "Rossi Paolo" → "Rossi P.": the box is small, the tooltip has it all.
  const short = (name: string) => {
    const w = name.trim().split(/\s+/);
    return w.length < 2 ? name : `${w[0]} ${w.slice(1).map((x) => `${x[0]}.`).join(' ')}`;
  };
  // A group keeps its whole name.
  const shortOf = (id: string | null | undefined, name: string) => (id?.startsWith('unit:') ? name : short(name));
  const occsOf = (g: ProgramGroup) => new Set([g.occurrence, ...g.days.map((d) => d.occurrence)]);

  // The week (group of events) on screen: the side list is about it.
  let current = $state(0);
  const group = $derived(p?.groups[Math.min(current, (p?.groups.length ?? 1) - 1)]);
  $effect(() => {
    if (!p) return;
    const on = () => {
      let i = 0;
      document.querySelectorAll('section.week').forEach((el, k) => {
        if (el.getBoundingClientRect().top <= 160) i = k;
      });
      current = i;
    };
    on();
    addEventListener('scroll', on, { passive: true });
    return () => removeEventListener('scroll', on);
  });

  // Drag and drop: a person from the side list onto a slot that accepts them.
  let dragging = $state<string | null>(null);
  const accepts = (c: Cell, id: string) => c.candidates.some((o) => o.id === id);
  const nameOf = (c: Cell, id: string) => c.candidates.find((o) => o.id === id)?.label ?? c.assignee_name ?? '';
  const DAY = 86_400_000;
  const dayOf = (iso: string) => Date.UTC(+iso.slice(0, 4), +iso.slice(5, 7) - 1, +iso.slice(8, 10));
  // Who can take a part of a week, by weeks since their last part before
  // it (other programs, then the earlier weeks of this one): never first.
  // The side list shows the week on screen; the phone picker, a slot's week.
  function peopleOf(g: ProgramGroup | undefined) {
    if (!p || !g) return [];
    const occs = occsOf(g);
    const from = dayOf(p.dates[g.occurrence] ?? '');
    const here = p.cells.filter((c) => occs.has(c.occurrence) && visible(c));
    const eligible = new Set(here.flatMap((c) => c.candidates.map((o) => o.id)));
    const taken = new Set(here.map(valueOf).filter(Boolean));
    const before = new Map<string, number>();
    for (const c of p.cells) {
      const v = valueOf(c);
      const d = dayOf(p.dates[c.occurrence] ?? '');
      if (v && d < from && (before.get(v) ?? -Infinity) < d) before.set(v, d);
    }
    // Their slots in the other programs sent, in the same week (Monday to Sunday).
    const monday = from - ((new Date(from).getUTCDay() + 6) % 7) * DAY;
    const sunday = monday + 6 * DAY;
    const dayLabel = (iso: string) =>
      new Date(dayOf(iso)).toLocaleDateString('it', { weekday: 'short', day: 'numeric', timeZone: 'UTC' });
    // The slot names on the line; program and day in the tooltip.
    const elsewhere = (id: string) =>
      p!.others.filter((o) => o.person === id && dayOf(o.date) >= monday && dayOf(o.date) <= sunday)
        .map((o) => ({ slot: o.slot, full: `${o.label} · ${dayLabel(o.date)}` }));
    return p.people
      .filter((x) => eligible.has(x.id))
      .map((x) => {
        const last = Math.max(x.last ? dayOf(x.last) : -Infinity, before.get(x.id) ?? -Infinity);
        const weeks = taken.has(x.id) ? 0 : last === -Infinity ? null : Math.floor((from - last) / (7 * DAY));
        return { ...x, weeks, taken: taken.has(x.id), elsewhere: elsewhere(x.id) };
      })
      .sort((a, b) => (b.weeks ?? Infinity) - (a.weeks ?? Infinity) || a.name.localeCompare(b.name, 'it', { sensitivity: 'base' }));
  }
  const people = $derived(peopleOf(group));

  // Phone: a tap on a slot opens the same list, limited to who can take it.
  let picking = $state<Cell | null>(null);
  const narrow = () => matchMedia('(max-width: 1000px)').matches;
  const groupOf = (c: Cell) => p?.groups.find((g) => occsOf(g).has(c.occurrence));
  const pickList = $derived(picking ? peopleOf(groupOf(picking)).filter((x) => accepts(picking!, x.id)) : []);
  // Candidates outside the people list (external speakers).
  const pickOthers = $derived(picking ? picking.candidates.filter((o) => !p?.people.some((x) => x.id === o.id)) : []);
  function pick(id: string) {
    if (picking) choose(picking, id);
    picking = null;
  }
  // The groups a slot of the program can go to (dragged like persons);
  // taken: already on a slot of the week on screen.
  function unitsOf(g: ProgramGroup | undefined) {
    if (!p) return [];
    const occs = g ? occsOf(g) : new Set<number>();
    const taken = new Set(p.cells.filter((c) => occs.has(c.occurrence) && visible(c)).map(valueOf).filter(Boolean));
    const seen = new Map<string, string>();
    for (const c of p.cells) if (visible(c)) for (const o of c.candidates) if (o.id.startsWith('unit:')) seen.set(o.id, o.label);
    return [...seen].map(([id, name]) => ({ id, name, taken: taken.has(id) })).sort((a, b) => a.name.localeCompare(b.name, 'it'));
  }
  const units = $derived(unitsOf(group));
  // Persons to drag beside the program (not when slots go to groups), and
  // the groups when a slot of the week goes to a group.
  const sidePeople = $derived(!!p && p.can_edit && !p.assign_units && p.people.length > 0);
  const side = $derived(!!p && p.can_edit && (sidePeople || units.length > 0));
  // The side column reaches down to the bar at the bottom wherever it is
  // (in place at the top of the page, then stuck under the header).
  let sideCol = $state<HTMLElement | null>(null);
  let sideTop = $state(84);
  $effect(() => {
    const el = sideCol;
    if (!el) return;
    const fit = () => (sideTop = Math.max(0, el.getBoundingClientRect().top));
    fit();
    // Capture: the page may scroll inside a container.
    addEventListener('scroll', fit, { passive: true, capture: true });
    addEventListener('resize', fit);
    return () => {
      removeEventListener('scroll', fit, { capture: true });
      removeEventListener('resize', fit);
    };
  });
  function drop(c: Cell, e: DragEvent) {
    e.preventDefault();
    const id = e.dataTransfer?.getData('text/plain') || dragging;
    dragging = null;
    if (!id) return;
    if (accepts(c, id)) choose(c, id);
    else toast(t('drop_not_allowed'), true);
  }

  async function load() {
    try {
      p = await run(readonly ? (sent ? api.sentProgram(program) : api.publishedProgram(program)) : api.program(program));
      // Types with a prefill: read the pages once, on first opening.
      if (!readonly && p.needs_prefill) await prefill(true);
    } catch (e) {
      fail(e);
    }
  }
  load();

  /** Reads the pages of all the events, or of one week (`g`). */
  async function prefill(quiet = false, g?: ProgramGroup) {
    reading = true;
    readingWeek = g?.occurrence ?? null;
    try {
      p = await run(api.prefillProgram(program, g ? [...occsOf(g)] : null));
      if (!quiet) toast(t('prefill_done'));
    } catch (e) {
      if (!quiet) fail(e);
    } finally {
      reading = false;
      readingWeek = null;
    }
  }

  function choose(c: Cell, person: string) {
    const k = keyOf(c);
    if (person === (c.assignee ?? '')) delete pending[k];
    else pending[k] = { occurrence: c.occurrence, slot: c.slot, index: c.index, person };
  }
  function write(c: Cell, id: string, value: string) {
    const k = keyOf(c);
    const d = details[k] ?? { occurrence: c.occurrence, slot: c.slot, index: c.index, values: {} };
    d.values[id] = value;
    details[k] = d;
  }
  // Dedicated days: rows added and removed at once (not with "Salva").
  let dSlot = $state('');
  let dDate = $state('');
  let dPerson = $state('');
  const dCandidates = $derived(p?.dedicated_slots.find((x) => x.id === (dSlot || p?.dedicated_slots[0]?.id))?.candidates ?? []);
  async function addDedicated() {
    const slot = dSlot || p?.dedicated_slots[0]?.id;
    if (!slot || !dDate || !dPerson) return;
    busy = true;
    try {
      p = await run(api.addDedicated(program, slot, dDate, dPerson));
      dPerson = '';
      toast(t('draft_saved'));
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }
  async function removeDedicated(r: { occurrence: number; slot: string; index: number }) {
    busy = true;
    try {
      p = await run(api.assignSlots(program, [{ occurrence: r.occurrence, slot: r.slot, index: r.index, person: '' }], [], []));
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }

  async function save(quiet = false) {
    if (!changed && !p?.unsent) return true;
    busy = true;
    try {
      p = await run(api.assignSlots(program, Object.values(pending), Object.values(details), Object.values(parts)));
      pending = {};
      details = {};
      parts = {};
      if (!quiet) toast(t('draft_saved'));
      return true;
    } catch (e) {
      fail(e);
      return false;
    } finally {
      busy = false;
    }
  }
  async function act(kind: 'send' | 'publish') {
    if (!(await save(true))) return;
    try {
      p = kind === 'send' ? await run(api.sendProgram(program)) : await run(api.publishProgram(program));
      toast(kind === 'send' ? t('sent') : t('saved'));
    } catch (e) {
      fail(e);
    }
  }
  async function addExternal() {
    try {
      await run(api.addExternal(external.trim()));
      external = '';
      p = await run(api.program(program));
    } catch (e) {
      fail(e);
    }
  }
</script>

{#snippet fields(c: Cell)}
  {#if c.details.length}
    <div class="details" class:changed={keyOf(c) in details}>
      {#each c.details as f (f.id)}
        {#if p?.can_edit}
          {#if f.kind === 'long_text'}
            <textarea class="wide" rows="2" placeholder={f.label} aria-label={`${c.label} · ${f.label}`} value={detailOf(c, f.id)} oninput={(e) => write(c, f.id, e.currentTarget.value)}></textarea>
          {:else}
            <input
              class={f.kind === 'integer' ? 'num' : 'grow'}
              type={f.kind === 'integer' ? 'number' : 'text'}
              min={f.min ?? undefined}
              placeholder={f.label}
              aria-label={`${c.label} · ${f.label}`}
              value={detailOf(c, f.id)}
              oninput={(e) => write(c, f.id, e.currentTarget.value)}
            />
          {/if}
        {:else if text(f.value)}
          <span class:muted={f.kind === 'long_text' || f.kind === 'integer'} class:lead={f.kind === 'text'} class:wide={f.kind === 'long_text'}>{f.id === 'minutes' ? `${f.value} min` : f.kind === 'integer' ? `${f.label} ${f.value}` : text(f.value)}</span>
        {/if}
      {/each}
    </div>
  {/if}
{/snippet}

<!-- A slot as a drop target: dashed when empty, with its label; a click opens the list. -->
<!-- mate: chosen with the row's first person (an assistant): "con …", smaller. -->
{#snippet zone(c: Cell, mate = false)}
  {#if readonly}
    {#if c.assignee}<div class="ro-name" class:mate title={c.assignee_name}>{#if mate}<span class="with">{c.prefix ?? t('with_person')}</span> {/if}{shortOf(c.assignee, c.assignee_name ?? '')}</div>{/if}
  {:else}
  {@const v = valueOf(c)}
  {@const ok = !!dragging && accepts(c, dragging)}
  <div
    class="zone"
    class:filled={!!v}
    class:changed={keyOf(c) in pending}
    class:can={ok}
    class:cannot={!!dragging && !ok}
    role="group"
    aria-label={c.label}
    ondragover={(e) => { if (ok) { e.preventDefault(); if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy'; } }}
    ondrop={(e) => drop(c, e)}
  >
    {#if p?.can_edit}
      <button type="button" class="tap" aria-label={c.label} onclick={() => { if (narrow()) picking = c; }}></button>
    {/if}
    {#if v}
      <span class="zname" title={nameOf(c, v)}>{shortOf(v, nameOf(c, v))}</span>
    {:else}
      <span class="zempty">{c.label}</span>
    {/if}
    {#if p?.can_edit}
      <select class="overlay" aria-label={c.label} value={v} onchange={(e) => choose(c, e.currentTarget.value)}>
        <option value="">—</option>
        {#each c.candidates as o (o.id)}<option value={o.id}>{o.label}</option>{/each}
      </select>
      {#if v}<button class="clear" aria-label={`${t('clear')} · ${c.label}`} onclick={() => choose(c, '')}>×</button>{/if}
    {/if}
    {#if c.assignee && !(keyOf(c) in pending) && c.response === 'declined'}<span class="zbadge badge warn">{t('response_declined')}</span>{/if}
  </div>
  {#if p?.can_edit && c.substitute && !(keyOf(c) in pending)}
    <!-- The substitute the assignee found, under the slot: one tap assigns them. -->
    <button type="button" class="subst btn small" onclick={() => choose(c, c.substitute!.id)}>{t('substitute_assign').replace('{name}', short(c.substitute.label))}</button>
  {/if}
  {/if}
{/snippet}

{#snippet rows(occurrence: number, perEvent: boolean)}
  <div class="rows" style={`--cols: ${1 + (p?.columns.length ?? 0)}`}>
    {#each rowsOf(occurrence, perEvent) as c, i (c.slot + c.index)}
      {@const prev = rowsOf(occurrence, perEvent)[i - 1]}
      {#if c.section && c.section !== prev?.section}
        {@const sec = sectionOf(c.section)}
        <div class="band" style={sec?.color ? `background: ${sec.color}` : ''}>{sec?.label}</div>
      {/if}
      <div class="part" class:inline={c.details.every((f) => f.kind === 'integer')} class:noname={!c.row_name}>
        {#if c.row_name}<span class="name">{c.label}</span>{/if}
        {@render fields(c)}
        {#if p?.can_edit}
          <button class="remove" title={t('part_remove')} aria-label={`${t('part_remove')} · ${c.label}`} onclick={() => setShown(c, false)}>−</button>
        {/if}
      </div>
      <div class="who">{#each inRow(c, null) as x, k (x.slot)}{#if x.assignable}{@render zone(x, k > 0)}{/if}{/each}</div>
      {#each p?.columns ?? [] as col (col.id)}
        <div class="who aux">{#each inRow(c, col.id) as x, k (x.slot)}{@render zone(x, k > 0)}{/each}</div>
      {/each}
    {/each}
  </div>
{/snippet}

{#if p}
  <div class="stack page wide" class:embedded={readonly}>
    {#if readonly && titled}<h2 class="part-title">{p.title.split(' · ')[0]}</h2>{/if}
    {#if !readonly}
      <div class="spread">
        <h1>{p.title}</h1>
        <span class="badge">{t(`status_${p.status}` as never)}</span>
      </div>
    {/if}
    {#if reading}<p class="muted" role="status">{t('prefill_reading')}</p>{/if}
    {#each p.warnings as w, i (i)}<p class="warn">{w}</p>{/each}
    <!-- Rows scroll away under this band, level with the sticky headers. -->
    <div class="topcover" aria-hidden="true"></div>
    <div class="editor" class:side>
      <div class="groups">
        {#each p.groups as g (g.occurrence)}
          <section class="card stack week">
            <!-- Sticks under the toolbar while the week's rows scroll; leaves with the card. -->
            <div class="whead" style={`--cols: ${1 + p.columns.length}`}>
              <div class="row wtitle">
                <div class="stack tight">
                  <h3>{g.label}</h3>
                  {#if g.detail}<span class="muted">{g.detail}</span>{/if}
                </div>
                {#if p.can_prefill}
                  <button class="btn small" disabled={busy || reading} onclick={() => prefill(false, g)}>{readingWeek === g.occurrence ? t('prefill_reading') : t('prefill')}</button>
                {/if}
              </div>
              {#if p.columns.length}
                <div class="head muted" title={t('main_column')}>{t('main_column')}</div>
                {#each p.columns as col (col.id)}<div class="head muted" title={col.label}>{col.label}</div>{/each}
              {/if}
            </div>
            {@render rows(g.occurrence, false)}
            {#each g.days as d (d.occurrence)}
              {#if rowsOf(d.occurrence, true).length}
                <h4 class="day">{d.label}</h4>
                {@render rows(d.occurrence, true)}
              {/if}
            {/each}
            {#if p.can_edit && addable(g).length}
              <select class="add-part" aria-label={t('part_add')} value="" onchange={(e) => { const c = addable(g).find((x) => keyOf(x) === e.currentTarget.value); if (c) setShown(c, true); e.currentTarget.value = ''; }}>
                <option value="">+ {t('part_add')}</option>
                {#each addable(g) as c (keyOf(c))}<option value={keyOf(c)}>{sectionOf(c.section)?.label ? `${sectionOf(c.section)?.label} · ` : ''}{c.label}</option>{/each}
              </select>
            {/if}
          </section>
        {/each}
      </div>
      {#if side}
        <aside class="side-col" class:both={sidePeople && units.length > 0} bind:this={sideCol} style={`height: calc(100vh - ${sideTop}px - 84px)`}>
          {#if sidePeople}
            <section class="people card" aria-label={t('people_side')}>
              <h3>{t('people_side')}</h3>
              {#if group}<p class="week-of">{group.label}</p>{/if}
              <p class="muted mini-hint">{t('people_side_hint')}</p>
              <ul>
                {#each people as x (x.id)}
                  <li
                    class="chip"
                    class:taken={x.taken}
                    draggable="true"
                    ondragstart={(e) => { dragging = x.id; e.dataTransfer?.setData('text/plain', x.id); if (e.dataTransfer) e.dataTransfer.effectAllowed = 'copy'; }}
                    ondragend={() => (dragging = null)}
                  >
                    <span class="cname">{x.name}</span>
                    {#if x.elsewhere.length}<span class="elsewhere" title={x.elsewhere.map((o) => o.full).join('\n')}>{x.elsewhere.map((o) => o.slot).join(' · ')}</span>{/if}
                    <span class="weeks">{x.taken ? t('weeks_now') : x.weeks === null ? t('weeks_never') : t('weeks_n').replace('{n}', String(x.weeks))}</span>
                  </li>
                {/each}
              </ul>
              <form class="external" onsubmit={(e) => { e.preventDefault(); if (external.trim()) addExternal(); }}>
                <input aria-label={t('add_external')} placeholder={t('add_external')} bind:value={external} />
                <button class="btn small" type="submit" disabled={!external.trim()}>{t('add')}</button>
              </form>
            </section>
          {/if}
          {#if units.length}
            <section class="people units-box card" aria-label={t('units_side')}>
              <h3>{t('units_side')}</h3>
              {#if group && !sidePeople}<p class="week-of">{group.label}</p>{/if}
              <p class="muted mini-hint">{t('units_side_hint')}</p>
              <ul>
                {#each units as x (x.id)}
                  <li
                    class="chip"
                    class:taken={x.taken}
                    draggable="true"
                    ondragstart={(e) => { dragging = x.id; e.dataTransfer?.setData('text/plain', x.id); if (e.dataTransfer) e.dataTransfer.effectAllowed = 'copy'; }}
                    ondragend={() => (dragging = null)}
                  >
                    <span class="cname">{x.name}</span>
                    {#if x.taken}<span class="weeks">{t('weeks_now')}</span>{/if}
                  </li>
                {/each}
              </ul>
            </section>
          {/if}
        </aside>
      {/if}
    </div>
    {#if p.dedicated.length || (p.can_edit && p.dedicated_slots.length)}
      <!-- Slots on dedicated days: rows of date, slot and person added by hand. -->
      <section class="card stack dedicated">
        <h3>{t('dedicated_title')}</h3>
        {#if p.dedicated.length}
          <table class="drows"><tbody>
            {#each p.dedicated as r (`${r.occurrence}-${r.slot}-${r.index}`)}
              <tr>
                <td class="ddate">{r.date_label}</td>
                <td>{r.label}</td>
                <td class="ro-name">{short(r.assignee)}</td>
                {#if p.can_edit}<td class="dact"><button class="remove-row" aria-label={`${t('part_remove')} · ${r.label} ${r.date_label}`} disabled={busy} onclick={() => removeDedicated(r)}>−</button></td>{/if}
              </tr>
            {/each}
          </tbody></table>
        {:else}
          <p class="muted">{t('dedicated_none')}</p>
        {/if}
        {#if p.can_edit && p.dedicated_slots.length}
          <form class="row dadd" onsubmit={(e) => { e.preventDefault(); addDedicated(); }}>
            {#if p.dedicated_slots.length > 1}
              <select aria-label={t('dedicated_slot')} bind:value={dSlot}>
                {#each p.dedicated_slots as x (x.id)}<option value={x.id}>{x.label}</option>{/each}
              </select>
            {:else}
              <span class="dslot">{p.dedicated_slots[0].label}</span>
            {/if}
            <input type="date" aria-label={t('dedicated_date')} bind:value={dDate} />
            <select aria-label={t('dedicated_person')} bind:value={dPerson}>
              <option value="">{t('dedicated_person')}</option>
              {#each dCandidates as c (c.id)}<option value={c.id}>{c.label}</option>{/each}
            </select>
            <button class="btn" type="submit" disabled={busy || !dDate || !dPerson}>{t('add')}</button>
          </form>
        {/if}
      </section>
    {/if}
    {#if !readonly}
    <div class="row actions">
      {#if p.can_edit}
        <button class="btn" disabled={busy || !changed} onclick={() => save()}>{t('save')}</button>
      {/if}
      {#if p.can_send}<button class="btn primary" disabled={busy || (!changed && !p.unsent)} onclick={() => act('send')}>{t('send_short')}</button>{/if}
      {#if p.can_publish}<button class="btn" disabled={busy} onclick={() => act('publish')}>{t('publish')}</button>{/if}
      {#if changed}<span class="muted" role="status">{t('unsaved_changes').replace('{n}', String(changed))}</span>{/if}
    </div>
    {/if}
    {#if picking}
      {@const pc = picking}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="sheet-backdrop" onclick={() => (picking = null)}></div>
      <div class="sheet" role="dialog" aria-modal="true" aria-label={pc.label} tabindex="-1" onkeydown={(e) => e.key === 'Escape' && (picking = null)}>
        <div class="sheet-head">
          <div class="stack tight">
            <strong>{pc.label}</strong>
            <span class="week-of">{groupOf(pc)?.label}</span>
          </div>
          <button class="btn small" onclick={() => (picking = null)}>{t('close')}</button>
        </div>
        <ul>
          {#if valueOf(pc)}<li><button class="chip none" onclick={() => pick('')}><span class="cname">{t('nobody')}</span></button></li>{/if}
          {#each pickList as x (x.id)}
            <li>
              <button class="chip" class:taken={x.taken} class:current={valueOf(pc) === x.id} onclick={() => pick(x.id)}>
                <span class="cname">{x.name}</span>
                {#if x.elsewhere.length}<span class="elsewhere">{x.elsewhere.map((o) => o.slot).join(' · ')}</span>{/if}
                <span class="weeks">{x.taken ? t('weeks_now') : x.weeks === null ? t('weeks_never') : t('weeks_n').replace('{n}', String(x.weeks))}</span>
              </button>
            </li>
          {/each}
          {#each pickOthers as o (o.id)}
            <li><button class="chip" onclick={() => pick(o.id)}><span class="cname">{o.label}</span></button></li>
          {/each}
        </ul>
      </div>
    {/if}
    <!-- With the side list it sits at its bottom (here only where the list is hidden). -->
    {#if p.can_edit}
      <div class="row" class:narrow-only={side}>
        <input aria-label={t('add_external')} placeholder={t('add_external')} bind:value={external} style="max-width: 280px" />
        <button class="btn" disabled={!external.trim()} onclick={addExternal}>{t('add')}</button>
      </div>
    {/if}
  </div>
{/if}

<style>
  .page { gap: 16px; }
  .tight { gap: 2px; }
  .groups { display: flex; flex-direction: column; gap: 16px; }
  .details { display: flex; flex-wrap: wrap; gap: 8px; padding: 0 0 8px; }
  .details input, .details textarea { min-height: 34px; font-size: 14px; }
  .details input.grow { flex: 1 1 220px; min-width: 0; }
  .details .wide { flex: 1 1 100%; }
  .details textarea { resize: vertical; font: inherit; font-size: 14px; }
  .editor { width: 100%; }
  .editor.side { display: grid; grid-template-columns: minmax(0, 1fr) 260px; gap: 18px; align-items: start; }
  /* Height set from its place: down to the bar at the bottom (72px) and a gap. */
  .side-col { position: sticky; top: 84px; min-height: 240px; display: flex; flex-direction: column; gap: 12px; }
  .people { padding: 14px; display: flex; flex-direction: column; gap: 8px; flex: 1 1 0; min-height: 0; overflow-y: auto; }
  /* Both boxes: persons three quarters, groups one quarter. */
  .side-col.both > .people { flex-grow: 3; }
  .side-col.both > .units-box { flex-grow: 1; }
  .people > * { flex-shrink: 0; }
  .people h3 { font-size: 16px; }
  .week-of { margin: -4px 0 0; font-size: 13px; font-weight: 600; color: var(--primary-strong); }
  .mini-hint { font-size: 12px; margin: 0; }
  .people ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .chip { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; column-gap: 8px; padding: 8px 10px; border: 1px solid var(--border); border-radius: 10px; background: var(--bg); cursor: grab; font-size: 14px; user-select: none; }
  .chip:active { cursor: grabbing; }
  .chip.taken { opacity: 0.55; }
  .cname { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .weeks { grid-column: 2; grid-row: 1; font-size: 12px; color: var(--muted); }
  /* Other slots of the same week, one small line under the name. */
  .elsewhere { grid-column: 1 / -1; grid-row: 2; font-size: 11px; color: var(--warn, #b26a00); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; margin-top: 2px; }
  /* Every box the same height, so the rows of boxes line up. */
  .zone { position: relative; height: 64px; flex: none; border: 1.5px dashed var(--line); border-radius: 10px; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; padding: 4px 22px; gap: 1px; }
  .zone.filled { border-style: solid; background: var(--bg); }
  .zone.changed { border-color: var(--primary); box-shadow: 0 0 0 2px var(--primary-soft); }
  .zone.can { border-color: var(--ok, #2e7d32); background: color-mix(in srgb, var(--ok, #2e7d32) 10%, transparent); }
  .zone.cannot { opacity: 0.45; }
  .zempty { font-size: 12px; color: var(--muted); line-height: 1.2; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .zname { font-size: 13px; font-weight: 400; font-style: italic; line-height: 1.2; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .overlay { position: absolute; inset: 0; opacity: 0; cursor: pointer; width: 100%; height: 100%; min-height: 0; }
  .clear { position: absolute; top: 2px; right: 4px; z-index: 2; border: none; background: none; color: var(--muted); font-size: 16px; cursor: pointer; line-height: 1; padding: 2px 4px; }
  .zbadge { position: absolute; bottom: -8px; right: 6px; font-size: 10px; padding: 0 6px; pointer-events: none; }
  .external { display: flex; flex-direction: column; gap: 6px; margin-top: 6px; padding-top: 10px; border-top: 1px solid var(--border); }
  .external input { min-height: 38px; font-size: 14px; }
  .narrow-only { display: none; }
  @media (max-width: 1000px) { .editor.side { grid-template-columns: minmax(0, 1fr); } .side-col { display: none; } .narrow-only { display: flex; } }
  /* A transparent button over the box: on phones it opens the picker sheet. */
  .tap { display: none; position: absolute; inset: 0; z-index: 1; background: none; border: none; cursor: pointer; }
  @media (max-width: 1000px) { .tap { display: block; } .overlay { display: none; } }
  .sheet-backdrop { position: fixed; inset: 0; background: rgb(0 0 0 / 0.3); z-index: 40; }
  .sheet { position: fixed; left: 0; right: 0; bottom: 0; z-index: 41; max-height: 78vh; overflow-y: auto; background: var(--bg); border-radius: 18px 18px 0 0; padding: 14px 16px calc(18px + var(--inset-bottom, env(safe-area-inset-bottom, 0px))); box-shadow: 0 -10px 30px rgb(0 0 0 / 0.18); display: flex; flex-direction: column; gap: 10px; }
  .sheet-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; position: sticky; top: -14px; background: var(--bg); padding: 4px 0 6px; }
  .sheet ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .sheet .chip { width: 100%; text-align: left; font: inherit; font-size: 15px; color: var(--text); cursor: pointer; min-height: 48px; }
  .sheet .chip.current { border-color: var(--primary); background: var(--primary-soft); }
  .sheet .chip.none .cname { color: var(--muted); }
  /* Parts in rows: 4/5 the part, 1/5 per room. */
  .rows { display: grid; grid-template-columns: minmax(0, 4fr) repeat(var(--cols), minmax(0, 1fr)); }
  .rows > * { padding: 10px 0; border-bottom: 1px solid var(--border); }
  /* Gaps as padding, so the row lines run across every column. */
  .rows > .who { padding-left: 14px; }
  .topcover { position: fixed; top: 0; left: 0; right: 0; height: 84px; background: var(--page); z-index: 4; pointer-events: none; }
  @media (max-width: 760px) { .topcover { display: none; } }
  .whead { position: sticky; top: 84px; z-index: 3; display: grid; grid-template-columns: minmax(0, 4fr) repeat(var(--cols), minmax(0, 1fr)); align-items: end; background: var(--bg); margin: -18px -20px 0; padding: 18px 20px 10px; border-bottom: 1px solid var(--border); border-radius: var(--radius) var(--radius) 0 0; }
  /* One line, left-aligned with the names below, cut with "…" when narrow. */
  .whead .head { padding-left: 14px; min-width: 0; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .head { font-size: 13px; font-weight: 600; }
  .wtitle { gap: 12px; align-items: center; flex-wrap: nowrap; }
  .part { position: relative; padding-right: 30px; }
  .part.noname { padding-top: 10px; }
  /* Read-only: compact rows, titles in plain text. */
  .embedded .name { min-height: 0; }
  .embedded .rows > * { padding: 12px 0; }
  .embedded .rows > .who { padding-left: 14px; }
  /* Parts start where the band's title starts, with room above. */
  .embedded .rows > .part { padding: 16px 0 12px 12px; }
  .embedded .rows > .band { padding: 6px 12px; }
  /* Names at the bottom of the row, the assistant right under the student. */
  .embedded .who { justify-content: flex-end; gap: 0; }
  .embedded .details { row-gap: 4px; }
  .embedded .details { column-gap: 12px; }
  .lead { font-size: 15px; }
  /* Participants: plain weight, in italics. */
  .ro-name { min-height: 24px; display: flex; align-items: center; gap: 4px; font-weight: 400; font-style: italic; font-size: 15px; }
  .ro-name.mate { font-size: 13px; min-height: 20px; }
  .ro-name .with { color: var(--muted); font-style: normal; }
  .remove { position: absolute; top: 8px; right: 2px; width: 24px; height: 24px; border-radius: 50%; border: 1px solid var(--border); background: var(--bg); color: var(--muted); font-size: 16px; line-height: 1; cursor: pointer; opacity: 0; transition: opacity 0.1s; padding: 0; }
  .rows > .part:hover .remove, .remove:focus-visible { opacity: 1; }
  @media (hover: none) { .remove { opacity: 1; } }
  .remove:hover { color: var(--danger); border-color: var(--danger); }
  .add-part { margin-top: 10px; max-width: 320px; font-size: 14px; min-height: 38px; color: var(--muted); }
  .band { grid-column: 1 / -1; background: var(--muted); color: #fff; font-weight: 700; text-transform: uppercase; letter-spacing: 0.02em; font-size: 14px; padding: 6px 12px; margin-top: 10px; border-radius: 4px; border-bottom: none; }
  .part { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .part .details { padding: 0; }
  .part.inline { flex-direction: row; align-items: center; gap: 10px; }
  /* As tall as a menu, so the name and the first choice share a line. */
  /* Part titles in plain weight. */
  .name { font-weight: 400; min-height: 64px; display: flex; align-items: center; }
  .part.inline .details input.num { width: 72px; }
  /* Boxes sit at the bottom of their row, level with the part's last field. */
  .who { display: flex; flex-direction: column; justify-content: flex-end; gap: 8px; min-width: 0; }
  @media (max-width: 760px) {
    .rows { grid-template-columns: minmax(0, 1fr); }
    .whead { grid-template-columns: minmax(0, 1fr); top: 0; }
    .whead .head { display: none; }
    .rows > .who { border-bottom: none; padding-top: 0; padding-left: 0; }
    .rows > .who:empty { display: none; }
    .rows > .part { border-bottom: none; }
    /* Read-only: the name right under the part's title. */
    .embedded .rows > .part { padding-bottom: 4px; }
    .embedded .ro-name { min-height: 0; }
  }
  .details input.num { width: 84px; flex: none; }
  .details.changed input { border-color: var(--primary); }
  /* Down to the window's edge, so nothing shows below the buttons. */
  .actions { position: sticky; bottom: 0; z-index: 5; background: var(--page); padding: 10px 0 18px; }
  /* Phone: the bar's background runs behind the floating menu. */
  @media (max-width: 760px) { .actions { padding-bottom: calc(100px + var(--inset-bottom, env(safe-area-inset-bottom, 0px))); } }
  .part-title { margin: 8px 0 0; font-size: 20px; }
  .subst { align-self: stretch; margin-top: -2px; font-size: 13px; min-height: 0; padding: 6px 10px; white-space: nowrap; color: var(--primary-strong); border-color: var(--primary); background: var(--primary-soft); }
  .dedicated h3 { margin: 0; }
  .drows { width: 100%; border-collapse: collapse; }
  .drows td { padding: 10px 8px; border-bottom: 1px solid var(--border); vertical-align: middle; }
  .drows tr:last-child td { border-bottom: none; }
  .ddate { font-weight: 600; white-space: nowrap; }
  .dact { width: 36px; text-align: right; }
  .remove-row { width: 28px; height: 28px; border-radius: 50%; border: 1px solid var(--border); background: var(--bg); color: var(--muted); cursor: pointer; padding: 0; }
  .remove-row:hover { color: var(--danger); border-color: var(--danger); }
  .dadd { gap: 10px; flex-wrap: wrap; align-items: center; }
  .dadd select, .dadd input { width: auto; min-width: 160px; }
  .dslot { font-weight: 600; }
  .day { margin-top: 10px; font-size: 14px; color: var(--muted); font-weight: 600; }
</style>
