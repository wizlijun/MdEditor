import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// `tauri ios dev` sets TAURI_DEV_HOST to the Mac's LAN IP so the iPhone /
// iPad can reach the Vite dev server over Wi-Fi. On desktop dev this stays
// undefined and Vite binds to localhost as usual.
const host = process.env.TAURI_DEV_HOST

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // When host is set (mobile dev), listen on all interfaces so the device
    // can connect. Otherwise leave default (localhost-only).
    host: host || false,
    hmr: host
      ? { protocol: 'ws', host, port: 1421 }
      : undefined,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    target: 'safari15',
    minify: 'esbuild',
    sourcemap: false,
    rollupOptions: {
      // The Editor Kit is a *library-shaped* entry: plugins `import()` it and
      // call its exports. Vite's app default for this option drops entry
      // exports (HTML entries never need them), which tree-shook the whole kit
      // down to its CSS side effects. 'exports-only' keeps exports where an
      // entry has them and leaves the HTML entries untouched.
      preserveEntrySignatures: 'exports-only',
      input: {
        index: 'index.html',
        insights: 'insights.html',
        preview: 'preview.html',
        pluginMarket: 'plugin-market.html',
        logs: 'logs.html',
        dailyNotes: 'daily-notes.html',
        // Editor Kit: the editor component bundle handed to plugin windows at
        // runtime (spec §3.4). A JS entry with a stable, hash-free file name so
        // `plugin://<id>/__host__/assets/editor-kit-v1.js` can address it
        // forever (the `assets/` segment is required — `__host__/` maps onto
        // the host dist tree and only `dist/assets/` is reachable); it
        // shares the moraya / prosemirror chunks with the main window, so the
        // installer grows by ≈ 0.
        'editor-kit': 'src/editor-kit/main.ts',
      },
      output: {
        entryFileNames: (c) =>
          c.name === 'editor-kit' ? 'assets/editor-kit-v1.js' : 'assets/[name]-[hash].js',
        chunkFileNames: 'assets/[name]-[hash].js',
        assetFileNames: (a) => {
          const names = a.names ?? ((a as { name?: string }).name ? [(a as { name?: string }).name!] : [])
          return names.some((n) => n.startsWith('editor-kit'))
            ? 'assets/editor-kit-v1[extname]'
            : 'assets/[name]-[hash][extname]'
        },
      },
    },
  },
  optimizeDeps: {
    entries: ['index.html', 'insights.html', 'preview.html', 'plugin-market.html', 'logs.html', 'daily-notes.html'],
  },
})
