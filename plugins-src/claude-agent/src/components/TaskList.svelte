<script lang="ts">
  import type { Task } from '../lib/events'

  let { tasks, selected, onselect, label }:
    {
      tasks: Task[]
      selected: string | null
      onselect: (id: string) => void
      label: (k: string, v?: Record<string, string | number>) => string
    } = $props()

  // "2026-07-30T10:42:33+00:00" → "07-30 10:42"
  const when = (iso: string) => iso.slice(5, 16).replace('T', ' ')

  function status(task: Task): { text: string; kind: string } | null {
    if (task.running) return { kind: 'running', text: label('status.running') }
    if (!task.last_run) return null
    return {
      kind: task.last_run.status,
      text: `${label('status.' + task.last_run.status)} · ${when(task.last_run.started_at)}`,
    }
  }
</script>

<ul class="tasks">
  {#each tasks as task (task.id)}
    {@const st = status(task)}
    <li>
      <button class:active={task.id === selected} onclick={() => onselect(task.id)}>
        <span class="name">
          {task.name}
          {#if task.running}<span class="dot" title={label('status.running')}></span>{/if}
        </span>
        {#if st}
          <span class="state s-{st.kind}">{st.text}</span>
        {:else}
          <span class="desc">{task.description}</span>
        {/if}
      </button>
    </li>
  {/each}
</ul>

<style>
  .tasks { list-style: none; margin: 0; padding: 0; }
  /* A button inherits neither font-size nor font-family — declare both, or the
     row drifts out of alignment at larger UI font sizes. */
  button {
    font: inherit;
    font-size: 13px;
    display: block;
    width: 100%;
    text-align: left;
    padding: 7px 9px;
    background: none;
    border: 0;
    border-radius: 6px;
    color: inherit;
    cursor: pointer;
  }
  button:hover { background: color-mix(in srgb, currentColor 8%, transparent); }
  button.active { background: color-mix(in srgb, currentColor 14%, transparent); }
  .name { display: flex; align-items: center; gap: 5px; font-weight: 600; }
  .desc, .state {
    display: block;
    font-size: 11px;
    line-height: 1.35;
    margin-top: 2px;
  }
  .desc { opacity: 0.6; }
  .state { opacity: 0.7; font-variant-numeric: tabular-nums; }
  .s-error, .s-timeout { color: #d9534f; opacity: 0.9; }
  .s-running { opacity: 0.95; font-weight: 600; }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
    animation: pulse 1.4s ease-in-out infinite;
  }
  @keyframes pulse { 0%, 100% { opacity: 0.25 } 50% { opacity: 1 } }
  @media (prefers-reduced-motion: reduce) {
    .dot { animation: none; opacity: 0.7; }
  }
</style>
