import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
export default {
  preprocess: vitePreprocess(),
  kit: {
    // Single-page app: Tauri serves the built files; no server-side rendering.
    adapter: adapter({ pages: 'build', assets: 'build', fallback: 'index.html', strict: false })
  }
};
