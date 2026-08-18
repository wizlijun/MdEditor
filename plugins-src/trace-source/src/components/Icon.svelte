<!-- Icon.svelte — draws one icon from `lib/icons.ts`. Same arrangement as
     idea-spark's: the 24×24 viewBox, stroke-only rendering, `currentColor`,
     2px round strokes and `aria-hidden` are stated exactly once, here. The
     icons are decorative without exception — every caller carries the meaning
     in text or `aria-label`, so the `<svg>` is hidden from the tree.

     `{@html}` is safe for a closed reason: the bodies are module-level string
     constants in this plugin's own source, so no user, vault or agent content
     can ever reach it. -->
<script lang="ts">
  import { ICONS, type IconName } from '../lib/icons'

  const { name, size = 16 }: { name: IconName; size?: number } = $props()
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true">{@html ICONS[name]}</svg
>

<style>
  /* `inline` would put the icon on the text baseline and add descender space
     under it, which shows up as a crooked action bar. Callers align it. */
  svg {
    display: block;
    flex: 0 0 auto;
  }
</style>
