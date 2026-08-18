<!-- ConfirmDialog.svelte — the gate in front of an irreversible action.

     `lines` is the point of it: deleting an idea also deletes its `.proof.md`,
     and there is no trash to fish either back out of (design §5), so the dialog
     names every file that is about to go rather than asking about "this idea"
     in the abstract.

     Focus starts on Cancel, not on the destructive button: a stray Enter or
     Space arriving right after the dialog opens must not be what deletes a
     document. `Esc` cancels too, and focus is kept inside the dialog while it
     is open — see `onkeydown`. -->
<script lang="ts">
  import { onMount } from 'svelte'

  const {
    title,
    body,
    lines,
    confirmLabel,
    cancelLabel,
    onconfirm,
    oncancel,
  }: {
    title: string
    /** One sentence of context above the list (here: "this is permanent"). */
    body: string
    /** Explanatory lines, listed verbatim (here: the files that will be deleted). */
    lines: string[]
    confirmLabel: string
    cancelLabel: string
    onconfirm: () => void
    oncancel: () => void
  } = $props()

  let el: HTMLDivElement | undefined = $state()
  let cancelEl: HTMLButtonElement | undefined = $state()

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      oncancel()
      return
    }
    if (e.key !== 'Tab' || !el) return
    // Two focusable controls, so the trap is just "wrap at the ends".
    const focusable = Array.from(el.querySelectorAll<HTMLButtonElement>('button'))
    if (focusable.length === 0) return
    const first = focusable[0]
    const last = focusable[focusable.length - 1]
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault()
      last.focus()
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault()
      first.focus()
    }
  }

  onMount(() => cancelEl?.focus())
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={oncancel}></div>
<div
  bind:this={el}
  class="dialog"
  role="dialog"
  aria-modal="true"
  aria-labelledby="confirm-title"
  tabindex="-1"
  {onkeydown}
>
  <h2 id="confirm-title">{title}</h2>
  <p>{body}</p>
  <ul>
    {#each lines as line (line)}
      <li>{line}</li>
    {/each}
  </ul>
  <div class="actions">
    <button bind:this={cancelEl} type="button" class="ghost" onclick={oncancel}>{cancelLabel}</button>
    <button type="button" class="danger" onclick={onconfirm}>{confirmLabel}</button>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: rgb(0 0 0 / 0.28);
  }
  .dialog {
    position: fixed;
    z-index: 41;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: min(420px, calc(100vw - 2rem));
    box-sizing: border-box;
    padding: 1rem;
    border: 1px solid var(--line, #e5e7eb);
    border-radius: 10px;
    background: Canvas;
    color: CanvasText;
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.28);
  }
  .dialog:focus { outline: none; }
  h2 {
    margin: 0 0 0.4rem;
    font-size: 0.95rem;
  }
  p {
    margin: 0 0 0.6rem;
    font-size: 0.82rem;
    line-height: 1.45;
    opacity: 0.8;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
    max-height: 40vh;
    overflow-y: auto;
  }
  li {
    padding: 0.15rem 0;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.78rem;
    opacity: 0.8;
    word-break: break-all;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.4rem;
    margin-top: 0.9rem;
  }
  button {
    padding: 0.3rem 0.7rem;
    border-radius: 6px;
    font: inherit;
    font-size: 0.82rem;
    cursor: pointer;
  }
  .ghost {
    border: 1px solid var(--line, #d1d5db);
    background: none;
    color: inherit;
  }
  .danger {
    border: 1px solid transparent;
    background: #dc2626;
    color: #fff;
  }
</style>
