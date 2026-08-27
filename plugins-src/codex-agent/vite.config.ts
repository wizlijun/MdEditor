import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// Standalone plugin UI bundle. The host serves it under `plugin://<id>/…`, so
// asset URLs MUST be relative (`base: './'`). The build output `dist/` is
// copied verbatim into the installed plugin's `ui/` (scripts/dev-install-plugin.sh).
export default defineConfig({
  plugins: [svelte()],
  base: './',
  build: {
    target: 'safari15',
    minify: 'esbuild',
    sourcemap: false,
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: { input: { index: 'index.html' } },
  },
})
