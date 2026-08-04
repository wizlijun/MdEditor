import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// Standalone plugin UI bundle. Served by the host under `plugin://<id>/…`, so
// asset URLs MUST be relative (`base: './'`) — an absolute `/assets/…` would
// resolve to `plugin://<id>/assets/…` only by luck and breaks the traversal
// guard's `entry`-relative model. The build output `dist/` is copied verbatim
// into the installed plugin's `ui/` directory (see scripts/dev-install-plugin.sh).
export default defineConfig({
  plugins: [svelte()],
  base: './',
  build: {
    target: 'safari15',
    minify: 'esbuild',
    sourcemap: false,
    outDir: 'dist',
    emptyOutDir: true,
    // Single-page entry: index.html at the project root.
    rollupOptions: {
      input: { index: 'index.html' },
    },
  },
})
