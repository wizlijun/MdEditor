<script lang="ts">
  import { topicCount, type TopicCounts, type TopicDefinition } from '../lib/topics'
  import { t } from '../lib/strings'

  const {
    topics,
    counts = {},
    selectedId,
    disabled = false,
    onselect,
    onmanage,
  }: {
    topics: TopicDefinition[]
    counts?: TopicCounts
    selectedId: string | null
    disabled?: boolean
    onselect: (topicId: string) => void
    onmanage: () => void
  } = $props()
</script>

<section class="topic-bar" aria-labelledby="import-topic-title">
  <header>
    <div>
      <h2 id="import-topic-title">{t('topic.importTitle')}</h2>
      <p>{t('topic.required')}</p>
    </div>
    <button type="button" class="manage" onclick={onmanage}>{t('topic.manage')}</button>
  </header>

  {#if topics.length === 0}
    <button type="button" class="empty" onclick={onmanage}>
      {t('topic.emptySetup')}
    </button>
  {:else}
    <div class="cards" role="radiogroup" aria-label={t('topic.current')}>
      {#each topics as topic (topic.id)}
        <button
          type="button"
          class:selected={selectedId === topic.id}
          class="topic-card"
          role="radio"
          aria-checked={selectedId === topic.id}
          disabled={disabled}
          onclick={() => onselect(topic.id)}
        >
          <span class="label">{topic.label}</span>
          <span class="description" title={topic.description}>{topic.description}</span>
          <span class="count">{t('topic.bookCount', { count: topicCount(counts, topic.id) })}</span>
        </button>
      {/each}
    </div>
  {/if}
</section>

<style>
  .topic-bar {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    border: 1px solid color-mix(in srgb, var(--accent-color, #0a84ff) 35%, transparent);
    border-radius: 9px;
    background: color-mix(in srgb, var(--accent-color, #0a84ff) 5%, transparent);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  h2,
  p {
    margin: 0;
  }
  h2 {
    font-size: 12px;
    font-weight: 650;
  }
  header p {
    margin-top: 2px;
    font-size: 10px;
    opacity: 0.58;
  }
  button {
    font: inherit;
    color: inherit;
    cursor: pointer;
  }
  button:focus-visible {
    outline: 2px solid var(--accent-color, #0a84ff);
    outline-offset: 2px;
  }
  .manage {
    flex: none;
    padding: 3px 8px;
    border: 1px solid color-mix(in srgb, currentColor 20%, transparent);
    border-radius: 6px;
    background: color-mix(in srgb, currentColor 5%, transparent);
    font-size: 11px;
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 7px;
  }
  .topic-card {
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 3px 6px;
    padding: 8px 9px;
    text-align: left;
    border: 1px solid color-mix(in srgb, currentColor 16%, transparent);
    border-radius: 7px;
    background: color-mix(in srgb, currentColor 3%, transparent);
  }
  .topic-card:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--accent-color, #0a84ff) 60%, transparent);
    background: color-mix(in srgb, var(--accent-color, #0a84ff) 8%, transparent);
  }
  .topic-card.selected {
    border-color: var(--accent-color, #0a84ff);
    background: color-mix(in srgb, var(--accent-color, #0a84ff) 13%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent-color, #0a84ff) 24%, transparent);
  }
  .topic-card:disabled {
    cursor: default;
    opacity: 0.55;
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    font-weight: 650;
  }
  .description {
    grid-column: 1 / -1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 10px;
    opacity: 0.62;
  }
  .count {
    font-size: 10px;
    opacity: 0.55;
  }
  .empty {
    width: 100%;
    padding: 12px;
    border: 1px dashed color-mix(in srgb, currentColor 28%, transparent);
    border-radius: 7px;
    background: transparent;
    font-size: 11px;
    text-align: center;
  }
</style>
