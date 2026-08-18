<script lang="ts">
  // What is behind this window, stated before anything else: which harness,
  // which version, which model, and whether it is in a state to run at all.
  //
  // It exists because a version alone answers the wrong question. A harness with
  // expired credentials reports its version perfectly happily and then fails
  // every run — so the last run's environment-level failure is shown here too,
  // where it reads as "fix your login" rather than "this task is broken".
  import type { HarnessStatus } from '../lib/events'
  import type { MessageKey } from '../lib/strings'

  let { status, label }: {
    /** `undefined` = the probe has not answered yet; `null` = it answered with
     *  nothing. Rendering both as "checking…" is how a failed probe became a
     *  spinner that never stopped. */
    status: HarnessStatus | null | undefined
    label: (k: MessageKey, v?: Record<string, string | number>) => string
  } = $props()
</script>

<div
  class="banner"
  class:bad={status === null || status?.ok === false}
  class:warn={!!status?.warning}
>
  {#if status === undefined}
    <span class="name">{label('harness.probing')}</span>
  {:else if status === null}
    <span class="name">{label('harness.unknown')}</span>
  {:else if !status.ok}
    <span class="name">{status.harness}</span>
    <!-- "Not installed" and "installed but will not start" send the user to two
         completely different places. `origin` is the tell: we found something,
         it just could not run. Saying "not installed" about a harness sitting
         in a directory we just named sends them hunting for an install they
         already have. -->
    <span class="state">
      {label(status.origin ? 'harness.broken' : 'harness.missing')}
    </span>
    {#if status.hint}<span class="hint" title={status.hint}>{status.hint}</span>{/if}
    {#if status.origin}<span class="origin" title={status.origin}>{status.origin}</span>{/if}
  {:else}
    <span class="name">{status.harness}</span>
    {#if status.version}<span class="ver">{status.version}</span>{/if}
    {#if status.default_model}
      <span class="model">{label('harness.model', { model: status.default_model })}</span>
    {/if}
    {#if status.origin}<span class="origin" title={status.origin}>{status.origin}</span>{/if}
  {/if}
</div>

{#if status?.warning}
  <p class="alert" title={status.warning}>
    {label('harness.warning', { detail: status.warning })}
  </p>
{/if}

<style>
  .banner {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 6px;
    padding: 6px 9px;
    margin-bottom: 8px;
    border-radius: 7px;
    background: color-mix(in srgb, currentColor 7%, transparent);
    font-size: 11px;
    line-height: 1.4;
  }
  .banner.bad { background: color-mix(in srgb, #d9534f 16%, transparent); }
  .banner.warn { background: color-mix(in srgb, #b8860b 18%, transparent); }
  .name { font-weight: 600; font-size: 12px; }
  .ver, .model {
    font-family: ui-monospace, SFMono-Regular, monospace;
    opacity: 0.8;
  }
  .state { color: #d9534f; font-weight: 600; }
  /* The origin can be a long path; it earns a line but not the layout. */
  .origin, .hint {
    flex-basis: 100%;
    opacity: 0.6;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .alert {
    margin: -4px 0 8px;
    padding: 5px 9px;
    border-radius: 7px;
    background: color-mix(in srgb, #b8860b 16%, transparent);
    font-size: 11px;
    line-height: 1.45;
  }
</style>
