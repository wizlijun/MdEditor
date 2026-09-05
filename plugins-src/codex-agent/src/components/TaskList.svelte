<script lang="ts">
  import type { Task } from '../lib/events'
  import { fmtShort } from '../lib/datetime'
  import type { MessageKey } from '../lib/strings'
  import { groupTasks } from '../lib/task-groups'

  let { tasks, selected, onselect, label }:
    {
      tasks: Task[]
      selected: string | null
      onselect: (id: string) => void
      label: (k: MessageKey, v?: Record<string, string | number>) => string
    } = $props()

  // "2026-07-30T10:42:33Z" (UTC) → "07-30 18:42" in the user's local timezone
  const when = fmtShort
  let expanded = $state(new Set<string>())
  const groups = $derived(groupTasks(tasks))

  function toggle(id: string) { const next = new Set(expanded); next.has(id) ? next.delete(id) : next.add(id); expanded = next }
  function groupName(id: string): string {
    const known: Record<string, MessageKey> = {
      'notemd.core': 'tasks.group.core', 'notemd.ebook-import': 'tasks.group.ebook',
      'notemd.idea-spark': 'tasks.group.idea', 'notemd.memory': 'tasks.group.memory',
      'notemd.trace-source': 'tasks.group.trace', 'agent-tools': 'tasks.group.agent', custom: 'tasks.group.custom',
    }
    return known[id] ? label(known[id]) : id
  }

  function status(task: Task): { text: string; kind: string } | null {
    if (task.running) return { kind: 'running', text: label('status.running') }
    if (!task.last_run) return null
    return {
      kind: task.last_run.status,
      text: `${label(('status.' + task.last_run.status) as MessageKey)} · ${when(task.last_run.started_at)}`,
    }
  }
</script>

<div class="task-groups">
{#each groups as group, index (group.id)}
  {@const panelId = `task-group-${index}`}
  <div class="task-group">
    <button class="group-toggle" aria-expanded={expanded.has(group.id)} aria-controls={panelId} onclick={() => toggle(group.id)}>
      <span class="chevron" aria-hidden="true">›</span><span class="group-name">{groupName(group.id)}</span>
      {#if group.running}<span class="dot" title={label('status.running')}></span>{/if}<span class="count">{group.tasks.length}</span>
    </button>
    {#if expanded.has(group.id)}
    <ul class="tasks" id={panelId}>
  {#each group.tasks as task (task.id)}
    {@const st = status(task)}
    <li>
      <button class="task-button" class:active={task.id === selected} aria-current={task.id === selected ? 'page' : undefined} onclick={() => onselect(task.id)}>
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
    {/if}
  </div>
{/each}
</div>

<style>
  .task-groups { display: grid; gap: 5px; }
  .task-group { min-width: 0; }
  .group-toggle { display: flex; align-items: center; gap: 7px; width: 100%; padding: 7px 8px; border: 1px solid var(--window-border); border-radius: 9px; background: var(--card-surface); color: inherit; font: inherit; font-size: 12px; text-align: left; cursor: pointer; }
  .group-toggle:hover { border-color: var(--strong-border); background: var(--hover-surface); }
  .group-toggle:focus-visible { outline: 2px solid var(--standard-accent); outline-offset: 2px; }
  .chevron { color: var(--muted-text); font-size: 17px; line-height: 1; transition: transform 120ms ease; }
  .group-toggle[aria-expanded='true'] .chevron { transform: rotate(90deg); }
  .group-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 620; }
  .count { margin-left: auto; color: var(--muted-text); font-size: 12px; font-variant-numeric: tabular-nums; }
  .tasks { list-style: none; margin: 5px 0 2px 13px; padding: 0; }
  .tasks li + li { margin-top: 4px; }
  .task-button {
    font: inherit;
    font-size: 13px;
    display: block;
    width: 100%;
    text-align: left;
    padding: 9px 10px;
    background: var(--card-surface, transparent);
    border: 1px solid var(--window-border, color-mix(in srgb, currentColor 11%, transparent));
    border-radius: 10px;
    color: inherit;
    cursor: pointer;
  }
  .task-button:hover {
    border-color: var(--strong-border, color-mix(in srgb, currentColor 18%, transparent));
    background: var(--hover-surface, color-mix(in srgb, currentColor 5%, transparent));
  }
  .task-button.active {
    border-color: color-mix(in srgb, var(--standard-accent, #3479db) 45%, transparent);
    background: var(--ui-selection);
    box-shadow: inset 3px 0 0 var(--standard-accent, #3479db);
  }
  .task-button:focus-visible { outline: 2px solid var(--standard-accent, #3479db); outline-offset: 2px; }
  .name { min-width: 0; overflow-wrap: anywhere; display: flex; align-items: center; gap: 6px; font-weight: 650; }
  .desc, .state {
    display: block;
    font-size: 12px;
    line-height: 1.35;
    margin-top: 2px;
  }
  .desc { color: var(--muted-text, currentColor); }
  .state { color: var(--muted-text, currentColor); font-variant-numeric: tabular-nums; }
  .s-error, .s-timeout { color: var(--ui-danger); }
  .s-running { color: var(--ui-secondary); font-weight: 600; }
  .policy-error { display: block; margin-top: 3px; color: var(--ui-danger); font-size: 12px; }
  .mode {
    display: inline-block;
    margin-top: 3px;
    padding: 0 5px;
    border-radius: 4px;
    font-size: 12px;
    font-family: ui-monospace, SFMono-Regular, monospace;
    background: color-mix(in srgb, currentColor 10%, transparent);
    color: var(--ui-secondary);
  }
  .m-danger-full-access { color: var(--ui-danger); font-weight: 600; }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--standard-accent, #3479db);
    animation: pulse 1.4s ease-in-out infinite;
  }
  @keyframes pulse { 0%, 100% { opacity: 0.25 } 50% { opacity: 1 } }
  @media (prefers-reduced-motion: reduce) {
    .dot { animation: none; opacity: 0.7; }
  }
</style>
