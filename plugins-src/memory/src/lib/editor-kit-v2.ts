import { bridge } from './bridge'
import type { EditorIdProvider, MountDocumentEditor, ReplaceBlockOperation } from '../../../../src/editor-kit-v2/contract'

// One type-only contract: plugin bundles never import the Host runtime.
export type * from '../../../../src/editor-kit-v2/contract'

export function kitV2Url(): string {
  return `plugin://${bridge().pluginId}/__host__/assets/editor-kit-v2.js`
}

export async function loadKitV2(): Promise<MountDocumentEditor> {
  const mod = (await import(/* @vite-ignore */ kitV2Url())) as { documentEditorApiVersion?: unknown; mountDocumentEditor?: unknown }
  if (mod.documentEditorApiVersion !== 2 || typeof mod.mountDocumentEditor !== 'function') {
    throw new Error('当前 note.md 宿主的文档编辑接口不兼容。请先升级 note.md，再重新打开 Memory 共写文档。')
  }
  return mod.mountDocumentEditor as MountDocumentEditor
}

export function replaceOperation(
  ids: EditorIdProvider,
  blockId: string,
  expectedBlockRevision: string,
  markdown: string,
): ReplaceBlockOperation {
  return {
    kind: 'block.replace',
    operationId: ids.operationId(),
    target: { blockId, expectedBlockRevision },
    payload: { content: markdown },
  }
}
