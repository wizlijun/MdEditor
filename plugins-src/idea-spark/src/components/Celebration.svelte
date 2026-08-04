<!-- Celebration.svelte — the two-second confetti burst a finished argument earns.
     Driven purely by `store.celebrate`: the store raises the flag (applyRunDone
     on a successful run) and this component lowers it again when the animation
     is over. Pure CSS — no canvas, no animation library, no asset.

     Reactivity note: the timer is started from an `$effect`, but the write
     (`clearCelebrate`) happens later, inside the timeout callback, i.e. outside
     the effect's tracking context — a synchronous read-then-write of the same
     `$state` inside an effect self-invalidates into a loop that freezes the
     whole window (MEMORY feedback_svelte_effect_untrack). `untrack` wraps the
     call anyway as documentation of that intent. The effect's teardown clears
     a pending timer, so a second run can't be cut short by the first's timer. -->
<script lang="ts">
  import { untrack } from 'svelte'
  import { clearCelebrate, state as store } from '../lib/store.svelte'
  import { t } from '../lib/strings'

  /** Matches the CSS animation budget below; the burst never outstays its welcome. */
  const DURATION_MS = 2000
  /** Deterministic-enough spread: fixed offsets, so no re-render reshuffles it. */
  const PIECES = Array.from({ length: 24 }, (_, i) => ({
    left: (i * 4.1 + (i % 5) * 1.7) % 100,
    delay: (i % 7) * 60,
    hue: (i * 47) % 360,
  }))

  $effect(() => {
    if (!store.celebrate) return
    const id = setTimeout(() => untrack(() => clearCelebrate()), DURATION_MS)
    return () => clearTimeout(id)
  })
</script>

{#if store.celebrate}
  <div class="celebration" aria-live="polite">
    <div class="confetti" aria-hidden="true">
      {#each PIECES as p, i (i)}
        <span
          class="piece"
          style="left:{p.left}%; animation-delay:{p.delay}ms; background:hsl({p.hue} 80% 60%)"
        ></span>
      {/each}
    </div>
    <p class="banner">{t('celebrate')}</p>
  </div>
{/if}

<style>
  .celebration {
    position: fixed;
    inset: 0;
    z-index: 20;
    /* Decoration only: never swallow a click meant for the editor underneath. */
    pointer-events: none;
    overflow: hidden;
  }
  .confetti { position: absolute; inset: 0; }
  .piece {
    position: absolute;
    top: -12px;
    width: 8px;
    height: 14px;
    border-radius: 2px;
    opacity: 0;
    animation: fall 1.8s ease-in forwards;
  }
  @keyframes fall {
    0% { transform: translateY(0) rotate(0deg); opacity: 1; }
    100% { transform: translateY(110vh) rotate(540deg); opacity: 0; }
  }
  .banner {
    position: absolute;
    top: 1.2rem;
    left: 50%;
    transform: translateX(-50%);
    margin: 0;
    padding: 0.35rem 0.9rem;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent, #2563eb) 90%, transparent);
    color: #fff;
    font-size: 0.85rem;
    box-shadow: 0 6px 18px rgb(0 0 0 / 0.2);
  }
  /* Respect the user's motion preference: keep the banner, drop the confetti. */
  @media (prefers-reduced-motion: reduce) {
    .piece { display: none; }
  }
</style>
