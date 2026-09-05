<!-- ModeToggle.svelte — the rich/source pill that floats over the editor's
     top-right corner.

     COPIED, deliberately, from the main window's `src/components/ModeToggle.svelte`:
     markup, both 16×16 SVGs (eye = rich, `</>` = source) and every style rule
     are reproduced verbatim so the plugin window reads as the same product. A
     plugin runs in an isolated webview that cannot import the host's `src/`, so
     there is no way to share the component — **if the upstream file changes
     visually, this one must be updated by hand.**

     The only intentional divergences: the props are `mode`/`onchange` instead of
     a host `Tab` (this window has no tabs), the strings come from this plugin's
     own catalog, and there is no `tab.kind !== 'image'` guard (an idea is always
     markdown). -->
<script lang="ts">
  import { t } from '../lib/strings'

  let { mode, onchange }: { mode: 'rich' | 'source'; onchange: (m: 'rich' | 'source') => void } = $props()
</script>

<div class="seg" role="group" aria-label={t('editorMode')}>
  <button
    type="button"
    aria-pressed={mode === 'rich'}
    aria-label={t('modeRich')}
    class:active={mode === 'rich'}
    onclick={() => onchange('rich')}
    title={t('modeRich')}
  >
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/>
      <circle cx="12" cy="12" r="3"/>
    </svg>
  </button>
  <button
    type="button"
    aria-pressed={mode === 'source'}
    aria-label={t('modeSource')}
    class:active={mode === 'source'}
    onclick={() => onchange('source')}
    title={t('modeSource')}
  >
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <polyline points="16 18 22 12 16 6"/>
      <polyline points="8 6 2 12 8 18"/>
    </svg>
  </button>
</div>

<style>
  .seg {
    display: inline-flex;
    align-items: center;
    background: color-mix(in srgb, CanvasText 9%, Canvas);
    border-radius: 8px;
    padding: 2px;
    gap: 0;
  }
  .seg button {
    width: 32px;
    height: 26px;
    border: 0;
    background: transparent;
    color: CanvasText;
    border-radius: 6px;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    opacity: 0.5;
    transition: opacity 80ms;
  }
  .seg button:hover { opacity: 0.85; }
  .seg button.active {
    background: Canvas;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
    opacity: 1;
  }
  .seg svg { display: block; }
</style>
