import { describe, it, expect, beforeEach } from 'vitest'
import { SELF, env } from 'cloudflare:test'

interface TestEnv {
  INDEX: KVNamespace
  PKGS: R2Bucket
}
const testEnv = env as unknown as TestEnv

const SAMPLE_INDEX = JSON.stringify({
  plugins: [
    {
      id: 'notemd.md2pdf',
      version: '1.2.0',
      min_host: '>=0.1.0',
      archs: ['aarch64-apple-darwin'],
      size: 1024,
      sha256: { 'aarch64-apple-darwin': 'deadbeef' },
      name: 'Export to PDF',
      category: 'import-export',
      download: {
        'aarch64-apple-darwin':
          'https://plugins.notemd.net/api/download/notemd.md2pdf/1.2.0/aarch64-apple-darwin',
      },
    },
  ],
})

// Miniflare KV/R2 persist across tests within a run; clear the keys we touch so
// each test starts from a known state.
beforeEach(async () => {
  await testEnv.INDEX.delete('index')
  await testEnv.INDEX.delete('stats:notemd.md2pdf')
  await testEnv.PKGS.delete('notemd.md2pdf/1.2.0/aarch64-apple-darwin.notemdpkg')
  await testEnv.PKGS.delete('notemd.md2pdf/1.2.0/aarch64-apple-darwin.notemdpkg.minisig')
})

describe('GET /api/index.json', () => {
  it('returns the KV index verbatim with json + cache + CORS headers', async () => {
    await testEnv.INDEX.put('index', SAMPLE_INDEX)
    const r = await SELF.fetch('http://x/api/index.json')
    expect(r.status).toBe(200)
    expect(r.headers.get('content-type')).toContain('application/json')
    expect(r.headers.get('cache-control')).toBe('public, max-age=300')
    expect(r.headers.get('access-control-allow-origin')).toBe('*')
    const body = await r.json() as { plugins: { id: string }[] }
    expect(body.plugins[0].id).toBe('notemd.md2pdf')
  })

  it('returns {"plugins":[]} when KV is empty', async () => {
    const r = await SELF.fetch('http://x/api/index.json')
    expect(r.status).toBe(200)
    expect(await r.json()).toEqual({ plugins: [] })
  })

  it('405 for POST', async () => {
    const r = await SELF.fetch('http://x/api/index.json', { method: 'POST' })
    expect(r.status).toBe(405)
    expect(r.headers.get('allow')).toContain('GET')
  })
})

describe('GET /api/download/<id>/<version>/<arch>', () => {
  it('streams the package with octet-stream content-type', async () => {
    const bytes = new Uint8Array([1, 2, 3, 4, 5])
    await testEnv.PKGS.put('notemd.md2pdf/1.2.0/aarch64-apple-darwin.notemdpkg', bytes)
    const r = await SELF.fetch('http://x/api/download/notemd.md2pdf/1.2.0/aarch64-apple-darwin')
    expect(r.status).toBe(200)
    expect(r.headers.get('content-type')).toBe('application/octet-stream')
    const got = new Uint8Array(await r.arrayBuffer())
    expect(Array.from(got)).toEqual([1, 2, 3, 4, 5])
  })

  it('serves the .minisig sibling from the <...>.notemdpkg.minisig key', async () => {
    const sig = new TextEncoder().encode('untrusted comment: signature\nRWQ...\n')
    await testEnv.PKGS.put('notemd.md2pdf/1.2.0/aarch64-apple-darwin.notemdpkg.minisig', sig)
    const r = await SELF.fetch('http://x/api/download/notemd.md2pdf/1.2.0/aarch64-apple-darwin.minisig')
    expect(r.status).toBe(200)
    expect(r.headers.get('content-type')).toBe('application/octet-stream')
    expect(await r.text()).toContain('untrusted comment')
  })

  it('404 when the R2 object is absent', async () => {
    const r = await SELF.fetch('http://x/api/download/notemd.md2pdf/9.9.9/aarch64-apple-darwin')
    expect(r.status).toBe(404)
  })

  it('404 on a malformed / path-traversal id', async () => {
    const r = await SELF.fetch('http://x/api/download/..%2Fescape/1.0.0/x')
    expect(r.status).toBe(404)
  })

  it('405 for POST', async () => {
    const r = await SELF.fetch('http://x/api/download/notemd.md2pdf/1.2.0/aarch64-apple-darwin', {
      method: 'POST',
    })
    expect(r.status).toBe(405)
  })
})

describe('POST /api/stats/install', () => {
  it('increments the per-id counter and returns 200', async () => {
    const r = await SELF.fetch('http://x/api/stats/install', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ id: 'notemd.md2pdf', version: '1.2.0' }),
    })
    expect(r.status).toBe(200)
    expect(await r.json()).toEqual({ ok: true })
    expect(await testEnv.INDEX.get('stats:notemd.md2pdf')).toBe('1')

    await SELF.fetch('http://x/api/stats/install', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ id: 'notemd.md2pdf', version: '1.2.0' }),
    })
    expect(await testEnv.INDEX.get('stats:notemd.md2pdf')).toBe('2')
  })

  it('returns 200 even for a malformed body (fire-and-forget) without counting', async () => {
    const r = await SELF.fetch('http://x/api/stats/install', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: 'not json',
    })
    expect(r.status).toBe(200)
    expect(await r.json()).toEqual({ ok: true })
    expect(await testEnv.INDEX.get('stats:notemd.md2pdf')).toBeNull()
  })

  it('405 for GET', async () => {
    const r = await SELF.fetch('http://x/api/stats/install')
    expect(r.status).toBe(405)
    expect(r.headers.get('allow')).toContain('POST')
  })
})

describe('GET / (landing page)', () => {
  it('serves the HTML marketplace page with html content-type + CORS', async () => {
    const r = await SELF.fetch('http://x/')
    expect(r.status).toBe(200)
    expect(r.headers.get('content-type')).toContain('text/html')
    expect(r.headers.get('access-control-allow-origin')).toBe('*')
    const body = await r.text()
    expect(body).toContain('<!DOCTYPE html>')
    expect(body).toContain('/api/index.json')
    expect(body).toContain('Capture')
    expect(body).toContain('Reading')
    expect(body).toContain('Import & Export')
    expect(body).toContain("'capture-import':'capture'")
    expect(body).toContain('groupPlugins')
  })

  it('serves the same page at /index.html', async () => {
    const r = await SELF.fetch('http://x/index.html')
    expect(r.status).toBe(200)
    expect(r.headers.get('content-type')).toContain('text/html')
  })

  it('HEAD / returns 200 with no body', async () => {
    const r = await SELF.fetch('http://x/', { method: 'HEAD' })
    expect(r.status).toBe(200)
    expect(await r.text()).toBe('')
  })

  it('405 for POST /', async () => {
    const r = await SELF.fetch('http://x/', { method: 'POST' })
    expect(r.status).toBe(405)
    expect(r.headers.get('allow')).toContain('GET')
  })
})

describe('routing', () => {
  it('404 for an unknown path', async () => {
    const r = await SELF.fetch('http://x/api/nope')
    expect(r.status).toBe(404)
  })

  it('OPTIONS preflight returns 204 with CORS headers', async () => {
    const r = await SELF.fetch('http://x/api/index.json', { method: 'OPTIONS' })
    expect(r.status).toBe(204)
    expect(r.headers.get('access-control-allow-origin')).toBe('*')
    expect(r.headers.get('access-control-allow-methods')).toContain('GET')
  })
})
