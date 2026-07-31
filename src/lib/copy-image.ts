/**
 * Copy an image's bitmap (not its link/markdown) to the system clipboard so it
 * can be pasted as an image into other apps — chat, Photoshop, Notes, etc.
 *
 * The input is the already-rendered `<img>` element from the editor; its src may
 * be an `asset://` vault file, a `data:`/`blob:` URI, or a remote `http(s)` URL.
 */
export async function copyImageToClipboard(el: HTMLImageElement): Promise<void> {
  const bytes = await imageToPngBytes(el)
  const { writeImage } = await import('@tauri-apps/plugin-clipboard-manager')
  await writeImage(bytes)
}

async function imageToPngBytes(el: HTMLImageElement): Promise<Uint8Array> {
  // Primary path: draw the already-decoded <img> onto a canvas and read it back
  // as PNG. This normalises any source format (jpg/webp/gif/svg all become PNG,
  // which is what Tauri's Image::from_bytes decodes) and works for same-origin
  // sources (asset://, data:, blob:).
  try {
    return await canvasToPng(el, el.naturalWidth, el.naturalHeight)
  } catch {
    // Cross-origin (remote) images taint the canvas, so toBlob throws a
    // SecurityError. Re-fetch the bytes ourselves — the resulting blob is
    // same-origin, so decoding it via createImageBitmap yields a clean canvas.
    const src = el.currentSrc || el.src
    const blob = await (await fetch(src)).blob()
    const bitmap = await createImageBitmap(blob)
    try {
      return await canvasToPng(bitmap, bitmap.width, bitmap.height)
    } finally {
      bitmap.close()
    }
  }
}

function canvasToPng(
  source: CanvasImageSource,
  width: number,
  height: number,
): Promise<Uint8Array> {
  if (!width || !height) throw new Error('image not decoded')
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  const ctx = canvas.getContext('2d')
  if (!ctx) throw new Error('no 2d canvas context')
  ctx.drawImage(source, 0, 0)
  return new Promise((resolve, reject) => {
    // toBlob throws synchronously (SecurityError) on a tainted canvas.
    canvas.toBlob((blob) => {
      if (!blob) return reject(new Error('canvas.toBlob returned null'))
      blob.arrayBuffer().then((buf) => resolve(new Uint8Array(buf)), reject)
    }, 'image/png')
  })
}
