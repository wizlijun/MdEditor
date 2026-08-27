<script lang="ts">
  import type { Task } from '../lib/events'
  import { fmtShort } from '../lib/datetime'
  import type { MessageKey } from '../lib/strings'

  let { tasks, selected, onselect, label }:
    {
      tasks: Task[]
      selected: string | null
      onselect: (id: string) => void
      label: (k: MessageKey, v?: Record<string, string | number>) => string
    } = $props()

  // "2026-07-30T10:42:33Z" (UTC) → "07-30 18:42" in the user's local timezone
  const when = fmtShort

  function status(task: Task): { text: string; kind: string } | null {
    if (task.running) return { kind: 'running', text: label('status.running') }
    if (!task.last_run) return null
    return {
      kind: task.last_run.status,
      text: `${label(('status.' + task.last_run.status) as MessageKey)} · ${when(task.last_run.started_at)}`,
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
        {#if task.permission_mode}
          <!-- What this task is allowed to do, before Run is pressed. The mode
               is enforced by the Codex sandbox, so it belongs at the decision
               point rather than only in a past run's log. -->
          <span class="mode m-{task.permission_mode}" title={task.policy_rationale ?? ''}>
            {task.permission_mode}
          </span>
        {:else if task.policy_error}
          <span class="policy-error" title={task.policy_error}>{label('err.badPolicy')}</span>
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
  .policy-error { display: block; margin-top: 3px; color: #d9534f; font-size: 10px; }
  .mode {
    display: inline-block;
    margin-top: 3px;
    padding: 0 5px;
    border-radius: 4px;
    font-size: 10px;
    font-family: ui-monospace, SFMono-Regular, monospace;
    background: color-mix(in srgb, currentColor 10%, transparent);
    opacity: 0.7;
  }
  .m-danger-full-access { color: #d9534f; opacity: 0.95; font-weight: 600; }
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
