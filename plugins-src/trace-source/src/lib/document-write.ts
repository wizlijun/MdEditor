/** Serializes editor writes so a slower, older save can never land after a
 * newer one. A failed write does not poison later retries. */
export class DocumentWriteQueue {
  private tail: Promise<void> = Promise.resolve()

  constructor(private readonly write: (path: string, content: string) => Promise<unknown>) {}

  enqueue(path: string, content: string): Promise<void> {
    const next = this.tail.catch(() => undefined).then(async () => {
      await this.write(path, content)
    })
    this.tail = next
    return next
  }

  /** Waits for the latest queued write, if any. */
  drain(): Promise<void> {
    return this.tail
  }
}
