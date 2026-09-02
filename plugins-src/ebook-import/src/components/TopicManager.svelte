<script lang="ts">
  import {
    cloneTopics,
    createTopic,
    MAX_TOPICS,
    moveTopic,
    stageTopicRemoval,
    topicCount,
    validateTopics,
    type TopicCounts,
    type TopicDefinition,
    type TopicValidationCode,
  } from '../lib/topics'
  import { t, type MessageKey } from '../lib/strings'

  const {
    open,
    topics,
    counts = {},
    onsave,
    onclose,
  }: {
    open: boolean
    topics: TopicDefinition[]
    counts?: TopicCounts
    /** Deletions and migrations are committed atomically with the other edits. */
    onsave: (
      topics: TopicDefinition[],
      migrations: Record<string, string>,
    ) => void | Promise<void>
    onclose: () => void
  } = $props()

  let drafts = $state<TopicDefinition[]>([])
  let originalIds = $state<string[]>([])
  let migrations = $state<Record<string, string>>({})
  let saving = $state(false)
  let actionError = $state('')
  const validation = $derived(validateTopics(drafts))

  $effect(() => {
    if (open) {
      drafts = cloneTopics(topics)
      originalIds = topics.map((topic) => topic.id)
      migrations = {}
      saving = false
      actionError = ''
    }
  })

  const messages: Record<TopicValidationCode, MessageKey> = {
    required: 'topic.validation.required',
    too_few: 'topic.validation.tooFew',
    too_many: 'topic.validation.tooMany',
    invalid_id: 'topic.validation.invalidId',
    invalid_index_file: 'topic.validation.invalidIndex',
    duplicate_id: 'topic.validation.duplicateId',
    duplicate_label: 'topic.validation.duplicateLabel',
    duplicate_index_file: 'topic.validation.duplicateIndex',
    duplicate_term: 'topic.validation.duplicateTerm',
  }

  function fieldError(path: string): string {
    const error = validation.errors.find((item) => item.path === path)
    return error ? t(messages[error.code]) : ''
  }

  function addVocabulary(topicIndex: number) {
    drafts[topicIndex].vocabulary.push({ term: '', description: '' })
  }

  function removeVocabulary(topicIndex: number, entryIndex: number) {
    const entries = drafts[topicIndex].vocabulary
    if (entries.length <= 2) return
    entries.splice(entryIndex, 1)
  }

  function needsMigration(topicId: string): boolean {
    return topicCount(counts, topicId) > 0 || Object.values(migrations).includes(topicId)
  }

  function deleteDraft(topic: TopicDefinition) {
    actionError = ''
    const staged = stageTopicRemoval(drafts, originalIds, counts, migrations, topic.id)
    drafts = staged.topics
    migrations = staged.migrations
  }

  async function save() {
    if (!validation.valid || saving) return
    saving = true
    actionError = ''
    try {
      await onsave(cloneTopics(drafts), { ...migrations })
    } catch (error) {
      actionError = error instanceof Error ? error.message : String(error)
    } finally {
      saving = false
    }
  }
</script>

{#if open}
  <div class="backdrop">
    <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="topic-manager-title">
      <header>
        <div>
          <h2 id="topic-manager-title">{t('topic.manager.title')}</h2>
          <p>{t('topic.manager.hint')}</p>
        </div>
        <button type="button" class="quiet" onclick={onclose} aria-label={t('topic.manager.close')}>{t('topic.manager.close')}</button>
      </header>

      <div class="topic-list">
        {#each drafts as topic, topicIndex (topic)}
          <fieldset>
            <legend>
              <span>{topic.label || t('topic.manager.newTopic', { number: topicIndex + 1 })}</span>
              <span class="book-count">{t('topic.bookCount', { count: topicCount(counts, topic.id) })}</span>
            </legend>

            <div class="order-actions" aria-label={t('topic.manager.sort')}>
              <button
                type="button"
                class="quiet"
                disabled={topicIndex === 0}
                onclick={() => (drafts = moveTopic(drafts, topic.id, -1))}
              >{t('topic.manager.up')}</button>
              <button
                type="button"
                class="quiet"
                disabled={topicIndex === drafts.length - 1}
                onclick={() => (drafts = moveTopic(drafts, topic.id, 1))}
              >{t('topic.manager.down')}</button>
            </div>

            <div class="fields two-column">
              <label>
                <span>{t('topic.manager.id')}</span>
                <input
                  bind:value={topic.id}
                  disabled={originalIds.includes(topic.id)}
                  aria-invalid={!!fieldError(`topics.${topicIndex}.id`)}
                  title={originalIds.includes(topic.id) ? t('topic.manager.idLocked') : ''}
                />
                {#if fieldError(`topics.${topicIndex}.id`)}
                  <small class="field-error">{fieldError(`topics.${topicIndex}.id`)}</small>
                {/if}
              </label>
              <label>
                <span>{t('topic.manager.label')}</span>
                <input
                  bind:value={topic.label}
                  aria-invalid={!!fieldError(`topics.${topicIndex}.label`)}
                  placeholder={t('topic.manager.labelPlaceholder')}
                />
                {#if fieldError(`topics.${topicIndex}.label`)}
                  <small class="field-error">{fieldError(`topics.${topicIndex}.label`)}</small>
                {/if}
              </label>
            </div>

            <label>
              <span>{t('topic.manager.description')}</span>
              <textarea
                rows="2"
                bind:value={topic.description}
                aria-invalid={!!fieldError(`topics.${topicIndex}.description`)}
                placeholder={t('topic.manager.descriptionPlaceholder')}
              ></textarea>
              {#if fieldError(`topics.${topicIndex}.description`)}
                <small class="field-error">{fieldError(`topics.${topicIndex}.description`)}</small>
              {/if}
            </label>

            <label>
              <span>{t('topic.manager.index')}</span>
              <input
                bind:value={topic.index_file}
                aria-invalid={!!fieldError(`topics.${topicIndex}.index_file`)}
                placeholder={t('topic.manager.indexPlaceholder')}
              />
              {#if fieldError(`topics.${topicIndex}.index_file`)}
                <small class="field-error">{fieldError(`topics.${topicIndex}.index_file`)}</small>
              {/if}
            </label>

            <div class="vocabulary-head">
              <span>{t('topic.manager.vocabulary')}</span>
              <button type="button" class="quiet" onclick={() => addVocabulary(topicIndex)}>{t('topic.manager.addTerm')}</button>
            </div>
            <div class="vocabulary">
              {#each topic.vocabulary as entry, entryIndex}
                <div class="vocabulary-row">
                  <label>
                    <span class="sr-only">{t('topic.manager.term')} {entryIndex + 1}</span>
                    <input
                      bind:value={entry.term}
                      aria-invalid={!!fieldError(`topics.${topicIndex}.vocabulary.${entryIndex}.term`)}
                      placeholder={t('topic.manager.term')}
                    />
                  </label>
                  <label>
                    <span class="sr-only">{t('topic.manager.termDescription')} {entryIndex + 1}</span>
                    <input
                      bind:value={entry.description}
                      aria-invalid={!!fieldError(`topics.${topicIndex}.vocabulary.${entryIndex}.description`)}
                      placeholder={t('topic.manager.termDescription')}
                    />
                  </label>
                  <button
                    type="button"
                    class="quiet remove-term"
                    disabled={topic.vocabulary.length <= 2}
                    aria-label={`${t('topic.manager.removeTerm')} ${entry.term || entryIndex + 1}`}
                    onclick={() => removeVocabulary(topicIndex, entryIndex)}
                  >{t('topic.manager.removeTerm')}</button>
                </div>
                {#if fieldError(`topics.${topicIndex}.vocabulary.${entryIndex}.term`)}
                  <small class="field-error">{fieldError(`topics.${topicIndex}.vocabulary.${entryIndex}.term`)}</small>
                {/if}
                {#if fieldError(`topics.${topicIndex}.vocabulary.${entryIndex}.description`)}
                  <small class="field-error">
                    {fieldError(`topics.${topicIndex}.vocabulary.${entryIndex}.description`)}
                  </small>
                {/if}
              {/each}
            </div>

            <div class="delete-row">
              {#if needsMigration(topic.id)}
                <label class="migration">
                  <span>{t('topic.manager.migrate')}</span>
                  <select bind:value={migrations[topic.id]}>
                    <option value="">{t('topic.manager.chooseOther')}</option>
                    {#each drafts.filter((candidate) => candidate.id !== topic.id) as candidate}
                      <option value={candidate.id}>{candidate.label || candidate.id}</option>
                    {/each}
                  </select>
                </label>
              {/if}
              <button
                type="button"
                class="danger"
                disabled={
                  drafts.length <= 1 ||
                  (needsMigration(topic.id) && !migrations[topic.id])
                }
                onclick={() => deleteDraft(topic)}
              >{t('topic.manager.delete')}</button>
            </div>
          </fieldset>
        {/each}
      </div>

      <footer>
        <button
          type="button"
          class="add"
          disabled={drafts.length >= MAX_TOPICS}
          onclick={() => (drafts = createTopic(drafts))}
        >{t('topic.manager.add')}</button>
        <span class="limit">{drafts.length} / {MAX_TOPICS}</span>
        {#if !validation.valid}
          <span class="summary-error">{t('topic.manager.fix')}</span>
        {/if}
        {#if actionError}<span class="summary-error">{actionError}</span>{/if}
        <button type="button" class="quiet" onclick={onclose}>{t('action.cancel')}</button>
        <button type="button" class="primary" disabled={!validation.valid || saving} onclick={save}>
          {saving ? t('topic.manager.saving') : t('settings.save')}
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: flex;
    justify-content: flex-end;
    background: rgb(0 0 0 / 24%);
  }
  .sheet {
    width: min(620px, calc(100vw - 28px));
    height: 100%;
    display: flex;
    flex-direction: column;
    color: inherit;
    background: var(--background-color, Canvas);
    border-left: 1px solid color-mix(in srgb, currentColor 18%, transparent);
    box-shadow: -12px 0 30px rgb(0 0 0 / 16%);
  }
  header,
  footer {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 12px 14px;
  }
  header {
    justify-content: space-between;
    border-bottom: 1px solid color-mix(in srgb, currentColor 12%, transparent);
  }
  footer {
    border-top: 1px solid color-mix(in srgb, currentColor 12%, transparent);
  }
  h2,
  p {
    margin: 0;
  }
  h2 {
    font-size: 14px;
  }
  header p {
    margin-top: 2px;
    font-size: 10px;
    opacity: 0.55;
  }
  button,
  input,
  textarea,
  select {
    font: inherit;
    color: inherit;
  }
  button {
    cursor: pointer;
  }
  button:disabled {
    cursor: default;
    opacity: 0.4;
  }
  button:focus-visible,
  input:focus-visible,
  textarea:focus-visible,
  select:focus-visible {
    outline: 2px solid var(--accent-color, #0a84ff);
    outline-offset: 1px;
  }
  .topic-list {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 12px 14px;
  }
  fieldset {
    position: relative;
    margin: 0 0 12px;
    padding: 12px;
    border: 1px solid color-mix(in srgb, currentColor 16%, transparent);
    border-radius: 8px;
  }
  legend {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 0 5px;
    font-size: 12px;
    font-weight: 650;
  }
  .book-count {
    font-size: 10px;
    font-weight: 400;
    opacity: 0.55;
  }
  .order-actions {
    position: absolute;
    top: 7px;
    right: 8px;
    display: flex;
    gap: 4px;
  }
  .fields,
  label {
    display: grid;
    gap: 4px;
  }
  .fields.two-column {
    grid-template-columns: 1fr 1.5fr;
    gap: 9px;
  }
  fieldset > label,
  .fields {
    margin-bottom: 9px;
  }
  label > span,
  .vocabulary-head > span {
    font-size: 10px;
    opacity: 0.65;
  }
  input,
  textarea,
  select {
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
    padding: 5px 7px;
    border: 1px solid color-mix(in srgb, currentColor 22%, transparent);
    border-radius: 5px;
    background: color-mix(in srgb, currentColor 3%, transparent);
    font-size: 11px;
  }
  textarea {
    resize: vertical;
  }
  input:disabled {
    opacity: 0.58;
  }
  [aria-invalid='true'] {
    border-color: #c62828;
  }
  .field-error,
  .summary-error {
    color: #c62828;
    font-size: 10px;
  }
  .vocabulary-head,
  .delete-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .vocabulary {
    display: grid;
    gap: 5px;
    margin-top: 5px;
  }
  .vocabulary-row {
    display: grid;
    grid-template-columns: minmax(90px, 0.7fr) minmax(160px, 1.5fr) auto;
    gap: 6px;
  }
  .quiet,
  .add {
    padding: 3px 7px;
    border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
    border-radius: 5px;
    background: transparent;
    font-size: 10px;
  }
  .delete-row {
    margin-top: 11px;
    padding-top: 9px;
    border-top: 1px solid color-mix(in srgb, currentColor 9%, transparent);
  }
  .migration {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
  }
  .migration select {
    max-width: 180px;
  }
  .danger {
    margin-left: auto;
    padding: 4px 8px;
    border: 1px solid color-mix(in srgb, #c62828 45%, transparent);
    border-radius: 5px;
    background: transparent;
    color: #c62828;
    font-size: 10px;
  }
  .danger:hover:not(:disabled) {
    background: #c62828;
    color: #fff;
  }
  .primary {
    padding: 5px 12px;
    border: none;
    border-radius: 6px;
    background: var(--accent-color, #0a84ff);
    color: #fff;
  }
  footer .limit {
    font-size: 10px;
    opacity: 0.5;
  }
  footer .summary-error:first-of-type {
    margin-left: auto;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }
</style>
