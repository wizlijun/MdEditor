<!-- App.svelte — Idea Spark window shell (scaffold placeholder).
     The real capture UI lands in a later task; for now this only wires the
     bridge + locale so the window opens and shows a localized title.
     Declares `color-scheme: light dark` so this standalone plugin window
     follows the system appearance (project convention — otherwise Canvas
     system colors get pinned to light; see MEMORY reference_webview_color_scheme). -->
<script lang="ts">
  import { bridge } from './lib/bridge'
  import { setLocale, t } from './lib/strings'

  setLocale(bridge().locale)
</script>

<main class="app">
  <h1>{t('title')}</h1>
</main>

<style>
  :global(:root) {
    color-scheme: light dark;
  }
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: Canvas;
    color: CanvasText;
  }
  .app {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100vh;
    box-sizing: border-box;
  }
  h1 {
    margin: 0;
    font-size: 1.2rem;
  }
</style>
