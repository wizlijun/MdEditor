<script lang="ts">
  import { modalFocus } from '../../../../src/lib/ui/modal-focus'
  import type { LibraryBook } from '../lib/library'
  import { classificationProposalIsValid, type TopicClassificationProposal } from '../lib/topic-classification'
  import type { TopicDefinition } from '../lib/topics'
  import { t } from '../lib/strings'

  const {
    proposal,
    expectedBooks,
    books,
    topics,
    applying = false,
    onchange,
    onapply,
    oncancel,
  }: {
    proposal: TopicClassificationProposal
    expectedBooks: string[]
    books: LibraryBook[]
    topics: TopicDefinition[]
    applying?: boolean
    onchange: (book: string, topicId: string) => void
    onapply: () => void
    oncancel: () => void
  } = $props()

  const valid = $derived(classificationProposalIsValid(proposal, expectedBooks, topics))

  function bookName(rel: string): string {
    return books.find((book) => book.rel === rel || book.rel.endsWith(`/${rel}`))?.name ?? rel.split('/').at(-1) ?? rel
  }
</script>

<div class="backdrop">
  <div class="review" role="dialog" aria-modal="true" aria-busy={applying} aria-labelledby="classification-title" use:modalFocus={{ onClose: oncancel, canClose: () => !applying }}>
    <header>
      <h2 id="classification-title">{t('topic.classificationTitle')}</h2>
      <p>{t('topic.classificationHint')}</p>
    </header>

    <div class="assignments">
      {#each proposal.assignments as assignment (assignment.book)}
        <label>
          <span class="book">
            <strong>{bookName(assignment.book)}</strong>
            <small>{assignment.book}</small>
          </span>
          <select
            value={assignment.topic_id}
            aria-label={t('topic.chooseForBook', { name: bookName(assignment.book) })}
            disabled={applying}
            onchange={(event) => onchange(assignment.book, event.currentTarget.value)}
          >
            {#each topics as topic (topic.id)}
              <option value={topic.id}>{topic.label}</option>
            {/each}
          </select>
        </label>
      {/each}
    </div>

    <footer>
      <span class:error={!valid}>
        {valid
          ? t('topic.classificationAssigned', { count: proposal.assignments.length })
          : t('topic.classificationInvalid')}
      </span>
      <button class="secondary" disabled={applying} onclick={oncancel}>{t('action.cancel')}</button>
      <button class="primary" disabled={applying || !valid} onclick={onapply}>
        {applying
          ? t('topic.classificationApplying')
          : t('topic.classificationConfirm', { count: proposal.assignments.length })}
      </button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    display: grid;
    place-items: center;
    padding: 24px;
    background: color-mix(in srgb, #000 38%, transparent);
  }
  .review {
    width: min(720px, 100%);
    max-height: min(720px, calc(100vh - 48px));
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px;
    border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
    border-radius: 10px;
    background: Canvas;
    color: CanvasText;
    box-shadow: 0 20px 60px color-mix(in srgb, #000 35%, transparent);
  }
  h2,
  p {
    margin: 0;
  }
  h2 { font-size: 15px; }
  header p { margin-top: 4px; font-size: 12px; color: var(--ui-secondary); }
  .assignments {
    min-height: 0;
    overflow-y: auto;
    border-block: 1px solid color-mix(in srgb, currentColor 12%, transparent);
  }
  .assignments label {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(160px, 240px);
    align-items: center;
    gap: 12px;
    padding: 8px 2px;
    border-bottom: 1px solid color-mix(in srgb, currentColor 8%, transparent);
  }
  .assignments label:last-child { border-bottom: 0; }
  .book { min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .book strong,
  .book small { overflow-wrap: anywhere; }
  .book small { color: var(--ui-secondary); }
  select {
    min-width: 0;
    font: inherit;
    color: inherit;
    background: transparent;
    border: 1px solid color-mix(in srgb, currentColor 24%, transparent);
    border-radius: 6px;
    padding: 5px 7px;
  }
  footer { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  footer span { margin-right: auto; font-size: 12px; color: var(--ui-secondary); }
  footer span.error { color: var(--ui-danger); opacity: 1; }
  button { font: inherit; color: inherit; cursor: pointer; }
  button.secondary,
  button.primary {
    border: 1px solid color-mix(in srgb, currentColor 28%, transparent);
    border-radius: 6px;
    padding: 5px 12px;
  }
  button.secondary { background: transparent; }
  button.primary { background: var(--ui-accent); color: white; border-color: transparent; font-weight: 600; }
  button:disabled { cursor: default; opacity: 0.48; }
  @media (max-width: 560px) { .assignments label { grid-template-columns: minmax(0, 1fr); } }
</style>
