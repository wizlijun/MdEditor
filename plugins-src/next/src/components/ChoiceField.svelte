<script lang="ts">
  export interface ChoiceOption {
    label: string
    value: string
  }

  let {
    field,
    label,
    value = $bindable(),
    options,
    placeholder = '',
    type = 'text',
    multiline = false,
  }: {
    field: string
    label: string
    value: string
    options: readonly ChoiceOption[]
    placeholder?: string
    type?: 'text' | 'date'
    multiline?: boolean
  } = $props()
</script>

<div class="field">
  <label for={field}>{label}</label>
  <div class="choices" data-choices-for={field}>
    {#each options as option}
      <button
        type="button"
        class:selected={value === option.value}
        aria-pressed={value === option.value}
        onclick={() => value = option.value}
      >{option.label}</button>
    {/each}
  </div>
  {#if multiline}
    <textarea id={field} rows="2" bind:value {placeholder}></textarea>
  {:else}
    <input id={field} {type} bind:value {placeholder} />
  {/if}
</div>

<style>
  .field { display: grid; gap: 7px; }
  label { font-size: 12px; font-weight: 600; }
  .choices { display: flex; flex-wrap: wrap; gap: 6px; }
  .choices button { border: 1px solid var(--line); border-radius: 999px; background: var(--card); color: var(--muted-strong); padding: 6px 9px; font: inherit; font-size: 11.5px; font-weight: 600; cursor: pointer; }
  .choices button:hover { background: var(--hover); color: var(--fg); }
  .choices button.selected { border-color: var(--accent); background: var(--accent-soft); color: var(--accent); }
  textarea, input { width: 100%; box-sizing: border-box; border: 1px solid var(--line-strong); border-radius: 9px; background: var(--input); color: var(--fg); padding: 9px 10px; font: inherit; outline: none; }
  textarea { resize: vertical; min-height: 42px; }
  textarea:focus, input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
</style>
