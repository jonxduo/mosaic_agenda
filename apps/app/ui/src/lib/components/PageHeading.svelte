<script lang="ts">
  // Heading of a page reached from another: back, a line, the title and a
  // second line below. Always 52px tall, as the board's month bar, so the
  // arrow stays put moving between these pages.
  import BackButton from './BackButton.svelte';

  let { title, subtitle = '', onback }: { title: string; subtitle?: string; onback: () => void } = $props();
</script>

<div class="heading">
  <BackButton onclick={onback} />
  <div class="text">
    <h1>{title}</h1>
    {#if subtitle}<span class="muted when">{subtitle}</span>{/if}
  </div>
</div>

<style>
  .heading { display: flex; align-items: center; gap: 12px; min-height: 52px; }
  .text { display: flex; flex-direction: column; min-width: 0; }
  h1 { margin: 0; line-height: 1.1; }
  .when { font-size: 14px; line-height: 1.3; }
  .when::first-letter { text-transform: uppercase; }
</style>
