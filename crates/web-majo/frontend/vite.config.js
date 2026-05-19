import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// vite outputs to ../static/, where axum's ServeDir picks it up.
// emptyOutDir clears the React prototype on first build — by design,
// the prototype lived in static/ and is being replaced.
export default defineConfig({
  plugins: [svelte()],
  build: {
    outDir: '../static',
    emptyOutDir: true,
    sourcemap: true,
  },
  server: {
    port: 5173,
  },
});
