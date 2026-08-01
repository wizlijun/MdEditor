import pdfCss from '../../plugins-src/md2pdf/assets/pdf.css?raw'
import { activeTab } from './tabs.svelte'
import { renderTabAsInlineBody, buildPdfTitle, htmlEscape, CRITIC_CSS } from './plugins/host-render-html'
import { pushToast } from './toast.svelte'
import { t } from './i18n/store.svelte'

/**
 * Wrap an inline-body HTML fragment (from renderTabAsInlineBody) into a
 * self-contained A4 print document. Mirrors md2pdf's template.rs wrap_html so
 * printing and PDF export share the exact same stylesheet and structure.
 */
export function wrapPrintHtml(body: string, title: string): string {
  const t = htmlEscape(title)
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>${t}</title>
<style>${pdfCss}</style>
<style>${CRITIC_CSS}</style>
</head>
<body data-pdf-title="${t}">
${body}
</body>
</html>`
}

/**
 * Render the active tab to A4 HTML and open the system print dialog.
 * Markdown / code / html tabs are supported; image tabs (and "no tab") get a
 * toast and return — images don't go through the HTML render pipeline.
 */
export async function printActiveTab(): Promise<void> {
  // Only markdown / html / code render through the HTML pipeline. image and
  // spreadsheet (CSV) tabs have no HTML rendering, so printing them is a no-op.
  const PRINTABLE_KINDS = new Set(['markdown', 'html', 'code'])
  const tab = activeTab()
  if (!tab || !PRINTABLE_KINDS.has(tab.kind)) {
    pushToast({ level: 'info', message: t('print.nothingToPrint') })
    return
  }

  let doc: string
  try {
    const body = await renderTabAsInlineBody(tab)
    doc = wrapPrintHtml(body, buildPdfTitle(tab))
  } catch (e) {
    pushToast({ level: 'error', message: t('print.renderFailed'), detail: String(e) })
    return
  }

  const iframe = document.createElement('iframe')
  iframe.setAttribute('aria-hidden', 'true')
  iframe.style.cssText =
    'position:fixed;right:0;bottom:0;width:0;height:0;border:0;opacity:0;'
  iframe.srcdoc = doc

  let cleaned = false
  const cleanup = () => {
    if (cleaned) return
    cleaned = true
    iframe.remove()
  }

  iframe.onerror = cleanup

  iframe.onload = () => {
    const win = iframe.contentWindow
    if (!win) {
      cleanup()
      return
    }
    // Remove the iframe once printing finishes (or is cancelled). afterprint
    // fires on the iframe's own window; a timeout is the belt-and-braces path
    // in case the platform doesn't deliver it.
    win.addEventListener('afterprint', cleanup, { once: true })
    setTimeout(cleanup, 60_000)
    win.focus()
    win.print()
  }

  document.body.appendChild(iframe)
}
