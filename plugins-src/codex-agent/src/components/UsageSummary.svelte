<script lang="ts">
  import { hasTokenUsage, usageTotal, type Usage } from '../lib/events'
  import type { MessageKey } from '../lib/strings'

  let { usage, label }: {
    usage?: Usage | null
    label: (k: MessageKey, v?: Record<string, string | number>) => string
  } = $props()

  const n = (value: number) => new Intl.NumberFormat().format(value)
  const cost = $derived(
    usage?.cost
      ? label(
          usage.cost.kind === 'provider_reported' ? 'usage.costReported' : 'usage.costEstimated',
          { amount: usage.cost.amount_usd.toFixed(6) },
        )
      : '',
  )
  const costTitle = $derived(
    usage?.cost?.kind === 'list_price_estimate'
      ? `${label('usage.costDisclaimer')}${usage.cost.pricing_as_of ? ` (${usage.cost.pricing_as_of})` : ''}`
      : undefined,
  )
</script>

<div class="usage" data-usage-available={usage ? 'true' : 'false'}>
  {#if usage && hasTokenUsage(usage)}
    <span class="total">{label('usage.total', { n: n(usageTotal(usage)) })}</span>
    <span>{label('usage.input', { n: n(usage.input_tokens) })}</span>
    {#if usage.cache_read_tokens}<span>{label('usage.cacheRead', { n: n(usage.cache_read_tokens) })}</span>{/if}
    {#if usage.cache_write_tokens}<span>{label('usage.cacheWrite', { n: n(usage.cache_write_tokens) })}</span>{/if}
    <span>{label('usage.output', { n: n(usage.output_tokens) })}</span>
    {#if usage.reasoning_tokens}<span>{label('usage.reasoning', { n: n(usage.reasoning_tokens) })}</span>{/if}
    {#if usage.model}<span>{usage.model}</span>{/if}
  {:else}
    <span>{label('usage.unavailable')}</span>
  {/if}
  {#if cost}<span title={costTitle}>{cost}</span>{/if}
</div>

<style>
  .usage {
    display: flex;
    flex-wrap: wrap;
    gap: 5px 10px;
    padding: 7px 12px;
    border-top: 1px solid color-mix(in srgb, currentColor 10%, transparent);
    font-size: 10px;
    line-height: 1.35;
    opacity: 0.68;
    font-variant-numeric: tabular-nums;
  }
  .total { font-weight: 600; }
</style>
