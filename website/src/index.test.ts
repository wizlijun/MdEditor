import { afterEach, describe, expect, it, vi } from 'vitest'

const WINDOWS_INSTALLER =
  'https://github.com/wizlijun/note.md/releases/download/v6.811.1/note.md_6.811.1_x64-setup.exe'

afterEach(() => {
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
  vi.resetModules()
})

describe('/download worker route', () => {
  it('redirects Windows visitors to the newest available installer when latest.json is mac-only', async () => {
    const cache = new Map<string, Response>()
    vi.stubGlobal('caches', {
      default: {
        match: vi.fn(async (request: Request) => cache.get(request.url)?.clone()),
        put: vi.fn(async (request: Request, response: Response) => {
          cache.set(request.url, response.clone())
        }),
      },
    })
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url
      if (url.endsWith('/releases/latest/download/latest.json')) {
        return Response.json({
          version: '6.829.2',
          platforms: {
            'darwin-aarch64': {
              url: 'https://github.com/wizlijun/note.md/releases/download/v6.829.2/note.md-aarch64.app.tar.gz',
            },
          },
        })
      }
      if (url.endsWith('/releases?per_page=100&page=1')) {
        return Response.json([
          {
            tag_name: 'v6.811.1',
            draft: false,
            prerelease: false,
            assets: [{ name: 'note.md_6.811.1_x64-setup.exe', browser_download_url: WINDOWS_INSTALLER }],
          },
        ])
      }
      throw new Error(`unexpected fetch: ${url}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    const worker = (await import('./index.js')).default
    const pending: Promise<unknown>[] = []
    const response = await worker.fetch(
      new Request('https://notemd.net/download?os=windows', {
        headers: { 'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' },
      }),
      { ASSETS: { fetch: vi.fn() } },
      { waitUntil: (promise: Promise<unknown>) => pending.push(promise) },
    )
    await Promise.all(pending)

    expect(response.status).toBe(302)
    expect(response.headers.get('Location')).toBe(WINDOWS_INSTALLER)
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })
})
