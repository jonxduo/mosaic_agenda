<script lang="ts">
  // Start screen: the package's animated logo (`/package/splash.mp4`, ending
  // on `/package/splash.webp`) while the core opens. Without the files it
  // shows nothing; with "reduce motion" only the still image.
  // `intro`: first start, the whole animation plays (a tap skips it) and
  // `onend` is called when it is over.
  let { leaving = false, intro = false, onend }: { leaving?: boolean; intro?: boolean; onend?: () => void } = $props();
  const still = typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  let video = $state(!still);
  let image = $state(true);
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="splash" class:leaving aria-hidden="true" onclick={() => intro && onend?.()}>
  {#if video}
    <video
      src="/package/splash.mp4"
      poster="/package/splash.webp"
      autoplay
      muted
      playsinline
      preload="auto"
      onended={() => intro && setTimeout(() => onend?.(), 600)}
      onerror={() => {
        video = false;
        if (intro) onend?.();
      }}
    ></video>
  {:else if image}
    <img src="/package/splash.webp" alt="" onload={() => intro && setTimeout(() => onend?.(), 1200)} onerror={() => {
      image = false;
      if (intro) onend?.();
    }} />
  {/if}
</div>

<style>
  .splash {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    /* The light of the video, so its edges blend in. */
    background: radial-gradient(circle at 50% 45%, #f3f5f7 0%, #e6eaeb 70%, #dfe3e4 100%);
    z-index: 100;
    transition: opacity 0.35s ease;
  }
  .splash.leaving {
    opacity: 0;
    pointer-events: none;
  }
  video,
  img {
    width: min(78vw, 360px);
    aspect-ratio: 1;
    object-fit: cover;
    -webkit-mask-image: radial-gradient(circle, #000 55%, transparent 72%);
    mask-image: radial-gradient(circle, #000 55%, transparent 72%);
  }
</style>
