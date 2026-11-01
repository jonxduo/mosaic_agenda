<script lang="ts">
  import '$lib/app.css';
  import { installBrowserBridge } from '$lib/bridge';
  let { children } = $props();
  installBrowserBridge();
</script>

{@render children()}
