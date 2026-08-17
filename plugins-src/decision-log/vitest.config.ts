import { defineConfig } from 'vitest/config'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// Mirrors the root repo's vitest setup: the svelte plugin plus the `browser`
// resolve condition so component tests can `mount()` the client runtime
// (without it, imports resolve to svelte's server build and mount() throws).
export default defineConfig({
  plugins: [svelte({ hot: false })],
  resolve: {
    conditions: ['browser'],
  },
  test: {
    environment: 'node',
    globals: false,
    include: ['src/**/*.test.ts'],
  },
})
