import { Fragment, type Node as PmNode, type Schema } from 'prosemirror-model'
import { history, redo, undo } from 'prosemirror-history'
import { Plugin, TextSelection, type Command, type Selection, type SelectionBookmark, type Transaction } from 'prosemirror-state'
import { Mapping, Step, StepMap, StepResult, type Mappable } from 'prosemirror-transform'
import { nodeBlockId } from './identity'

type Block = { id: string; content: Fragment; from: number; to: number }

function blocks(doc: PmNode): Block[] {
  const result: Block[] = []
  const seen = new Set<string>()
  doc.forEach((node, from) => {
    const id = nodeBlockId(node)
    if (!id) throw new RangeError('EDITOR_KIT_V2_HISTORY_IDENTITY')
    const previous = result.at(-1)
    if (previous?.id === id) {
      previous.content = previous.content.append(Fragment.from(node))
      previous.to += node.nodeSize
    } else {
      if (seen.has(id)) throw new RangeError('EDITOR_KIT_V2_HISTORY_IDENTITY')
      result.push({ id, content: Fragment.from(node), from, to: from + node.nodeSize })
      seen.add(id)
    }
  })
  return result
}

function sameOrder(left: Block[], right: Block[]): boolean {
  return left.length === right.length && left.every((block, index) => block.id === right[index].id)
}

function replacementMap(before: PmNode, after: PmNode): StepMap {
  const start = before.content.findDiffStart(after.content)
  if (start === null) return StepMap.empty
  const end = before.content.findDiffEnd(after.content)!
  const overlap = start - Math.min(end.a, end.b)
  return new StepMap([start, end.a + Math.max(0, overlap) - start, end.b + Math.max(0, overlap) - start])
}

// Synchronous command scopes, not an undo stack. ProseMirror normally skips a
// failed inverse inside an event. A governed command must reject the WHOLE event
// instead, including any preceding successful inverse steps in that event.
const attempts: Array<{ failed: boolean }> = []

/** A semantic history inverse, never a replacement for normal editor steps. */
export class BlockEditStep extends Step {
  private positionMap: StepMap

  constructor(readonly before: PmNode, readonly after: PmNode, positionMap?: StepMap) {
    super()
    blocks(before)
    blocks(after)
    this.positionMap = positionMap ?? replacementMap(before, after)
  }

  apply(doc: PmNode): StepResult {
    const fail = (reason: string) => {
      for (const attempt of attempts) attempt.failed = true
      return StepResult.fail(`EDITOR_KIT_V2_HISTORY_CONFLICT: ${reason}`)
    }
    let current: Block[]
    try { current = blocks(doc) } catch { return fail('block identity changed') }
    const expected = blocks(this.before), desired = blocks(this.after)
    const expectedById = new Map(expected.map((block) => [block.id, block]))
    const desiredById = new Map(desired.map((block) => [block.id, block]))
    const currentById = new Map(current.map((block) => [block.id, block]))
    const changed = new Set([...expected, ...desired].filter((block) => {
      const previous = expectedById.get(block.id), next = desiredById.get(block.id)
      return !previous || !next || !previous.content.eq(next.content)
    }).map((block) => block.id))
    const structural = !sameOrder(expected, desired)
    if (structural && !sameOrder(current, expected)) return fail('document order changed')
    for (const id of changed) {
      const previous = expectedById.get(id), actual = currentById.get(id)
      if (previous ? !actual || !previous.content.eq(actual.content) : actual) return fail(`block ${id} changed`)
    }
    const resultBlocks = structural ? desired : current
    let content = Fragment.empty
    for (const block of resultBlocks) {
      content = content.append(changed.has(block.id) ? desiredById.get(block.id)!.content : currentById.get(block.id)!.content)
    }
    if (!doc.type.validContent(content)) return fail('invalid document structure')
    const result = doc.copy(content)
    // Ordinary monotonic replacement maps are intentional: an identity move
    // cannot be represented safely by a non-monotonic PM position map. Only
    // this step's data decision is identity-aware, not arbitrary range steps.
    if (structural) this.positionMap = replacementMap(doc, result)
    else this.positionMap = new StepMap(current.filter((block) => changed.has(block.id)).flatMap((block) => [
      block.from, block.to - block.from, desiredById.get(block.id)!.content.size,
    ]))
    return StepResult.ok(result)
  }

  getMap(): StepMap { return this.positionMap }

  invert(): BlockEditStep { return new BlockEditStep(this.after, this.before, this.positionMap.invert()) }

  map(_mapping: Mappable): BlockEditStep {
    // A deleted position may be the same block moved elsewhere. Revalidate the
    // current identity and expected body in apply, never trust range endpoints.
    return new BlockEditStep(this.before, this.after, this.positionMap)
  }

  toJSON() { return { stepType: 'notemdCdrBlockEdit', before: this.before.toJSON(), after: this.after.toJSON() } }

  static fromJSON(schema: Schema, json: { before: unknown; after: unknown }): BlockEditStep {
    return new BlockEditStep(schema.nodeFromJSON(json.before), schema.nodeFromJSON(json.after))
  }
}

Step.jsonID('notemdCdrBlockEdit', BlockEditStep)

type Endpoint = { id: string; offset: number }

function endpoint(doc: PmNode, position: number): Endpoint | null {
  const block = blocks(doc).find((item) => position >= item.from && position < item.to)
  return block ? { id: block.id, offset: position - block.from } : null
}

class BlockBookmark implements SelectionBookmark {
  constructor(readonly anchor: Endpoint | null, readonly head: Endpoint | null, readonly fallback: SelectionBookmark) {}
  map(mapping: Mappable): BlockBookmark { return new BlockBookmark(this.anchor, this.head, this.fallback.map(mapping)) }
  resolve(doc: PmNode): Selection {
    const current = blocks(doc)
    const locate = (point: Endpoint | null) => {
      const block = point && current.find((item) => item.id === point.id)
      return block && point ? Math.min(block.to - 1, block.from + point.offset) : null
    }
    const anchor = locate(this.anchor), head = locate(this.head)
    return anchor === null || head === null ? this.fallback.resolve(doc)
      : TextSelection.between(doc.resolve(anchor), doc.resolve(head))
  }
}

function withBlockBookmark(selection: Selection): Selection {
  if (!(selection instanceof TextSelection)) return selection
  let anchor: Endpoint | null, head: Endpoint | null
  // Commands also work on an ordinary (non-governed) Moraya schema. Only the
  // semantic history plugin requires complete IDs; a bookmark may fall back.
  try {
    anchor = endpoint(selection.$anchor.doc, selection.anchor)
    head = endpoint(selection.$head.doc, selection.head)
  } catch { return selection }
  const bookmark = new BlockBookmark(anchor, head, selection.getBookmark())
  return new Proxy(selection, { get(target, property) {
    return property === 'getBookmark' ? () => bookmark : Reflect.get(target, property, target)
  } })
}

function guarded(command: Command): Command {
  return (state, dispatch, view) => {
    const attempt = { failed: false }
    let candidate: Transaction | undefined
    attempts.push(attempt)
    try {
      const current = new Proxy(state, { get(target, property) {
        return property === 'selection' ? withBlockBookmark(target.selection) : Reflect.get(target, property, target)
      } })
      const available = command(current, (tr) => { candidate = tr }, view)
      if (!available || attempt.failed || !candidate?.docChanged) return false
      dispatch?.(candidate)
      return true
    } finally { attempts.pop() }
  }
}

export const governedUndo = guarded(undo)
export const governedRedo = guarded(redo)

/** Keep the existing PM history plugin/key/options; change only its input steps. */
export function withGovernedHistory(plugins: readonly Plugin[], isRemote: (tr: Transaction) => boolean): Plugin[] {
  const historyKey = history().spec.key
  return plugins.map((plugin) => {
    if (plugin.spec.key !== historyKey || !plugin.spec.state) return plugin
    const stateSpec = plugin.spec.state
    return new Plugin({
      ...plugin.spec,
      state: { ...stateSpec, apply(tr, value, oldState, newState) {
        const appended = tr.getMeta('appendedTransaction') as Transaction | undefined
        if (!tr.docChanged || isRemote(tr) || tr.getMeta(plugin) || tr.getMeta('addToHistory') === false
          || appended?.getMeta('addToHistory') === false) {
          return stateSpec.apply(tr, value, oldState, newState)
        }
        const step = new BlockEditStep(tr.before, tr.doc)
        const mapping = new Mapping([step.getMap()])
        // The shadow exists ONLY for history's public state.apply. Other
        // plugins and the view keep the real PM steps, selection and mapping.
        const shadow = new Proxy(tr, { get(target, property) {
          if (property === 'steps') return [step]
          if (property === 'docs') return [tr.before]
          if (property === 'mapping') return mapping
          return Reflect.get(target, property, target)
        } })
        const previous = new Proxy(oldState, { get(target, property) {
          return property === 'selection' ? withBlockBookmark(target.selection) : Reflect.get(target, property, target)
        } })
        return stateSpec.apply(shadow, value, previous, newState)
      } },
      props: { ...plugin.spec.props, handleDOMEvents: { ...plugin.spec.props?.handleDOMEvents,
        beforeinput(view, event) {
          const kind = (event as InputEvent).inputType
          const command = kind === 'historyUndo' ? governedUndo : kind === 'historyRedo' ? governedRedo : null
          if (!command) return plugin.props.handleDOMEvents?.beforeinput?.call(plugin, view, event) ?? false
          if (!view.editable) return false
          event.preventDefault()
          command(view.state, view.dispatch, view)
          return true
        },
      } },
    })
  })
}
