<script lang="ts">
  interface Task { id: string; name: string; description: string }
  let { tasks, selected, onselect }:
    { tasks: Task[]; selected: string | null; onselect: (id: string) => void } = $props()
</script>

<ul class="tasks">
  {#each tasks as task (task.id)}
    <li>
      <button class:active={task.id === selected} onclick={() => onselect(task.id)}>
        <span class="name">{task.name}</span>
        <span class="desc">{task.description}</span>
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
  .name { display: block; font-weight: 600; }
  .desc {
    display: block;
    opacity: 0.6;
    font-size: 11px;
    line-height: 1.35;
    margin-top: 2px;
  }
</style>
