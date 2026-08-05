<!-- Icon.svelte — draws one icon from `lib/icons.ts`.

     Everything the icon set has in common is stated here, exactly once: the
     24×24 `viewBox`, stroke-only rendering, `currentColor`, 2px round strokes,
     and `aria-hidden`. Only the rendered size varies — 16 in the action bar and
     the context menu, 12 for an inbox row's status badge — so a caller passes a
     name and (sometimes) a size, and never a copy of the path data.

     `currentColor` is the point of the whole arrangement: `failed` inherits the
     warning color and `delete` the danger color straight from the CSS around
     them, which is precisely what the emoji this set replaced could not do
     (a color bitmap on macOS ignores both the theme and the text color).

     ## Accessibility

     These icons are decorative *without exception*: every place one is used
     already carries the meaning in text — the row menu draws the icon beside
     its label, and the two action-bar icon buttons keep the `aria-label` and
     `title` they had when they were emoji. So the `<svg>` is `aria-hidden` and
     the component takes no label prop: offering one would invite a caller to
     put the meaning *only* in the icon.

     ## `{@html}`

     The bodies are module-level string constants in this plugin's own source —
     no user, vault or agent content ever reaches this — and the alternative (a
     ten-branch `{#if}` chain) has no exhaustiveness check, whereas
     `Record<IconName, string>` makes a missing icon a type error. -->
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
