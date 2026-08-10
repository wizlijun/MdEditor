// notemd.net edge worker: force HTTPS, resolve /download, then serve static
// assets. Runs before the asset router (run_worker_first) so plain-HTTP hits
// get a 301 to https instead of being served directly. Everything else is
// delegated to the [assets] binding, preserving html_handling / SPA fallback.

import {
  GH_REPO,
  RELEASES_PAGE,
  detectTarget,
  macDownloadUrl,
  windowsUrlFromManifest,
  windowsUrlFromReleases,
} from "./resolve-download.js";

const LATEST_JSON_URL = `https://github.com/${GH_REPO}/releases/latest/download/latest.json`;
const RELEASES_API_URL = `https://api.github.com/repos/${GH_REPO}/releases?per_page=30`;
const UA = "notemd-site-worker";

const MANIFEST_TTL_S = 300; // 5 min — keeps GitHub traffic to ~1 hit per POP per TTL
const RELEASES_TTL_S = 1800; // 30 min — only queried during a Windows lag window
const RESOLVED_TTL_S = 600; // 10 min — a resolved installer URL is stable
const LKG_TTL_S = 60 * 60 * 24 * 30; // 30 d — last known good, the failure net

// Per-isolate memory cache in front of the Cache API, so warm isolates skip
// even the local cache lookup.
const mem = new Map(); // cache name → { value, expiresAt }

function memGet(name) {
  const hit = mem.get(name);
  if (hit && hit.expiresAt > Date.now()) return hit.value;
  mem.delete(name);
  return null;
}

function memPut(name, value, ttlS) {
  mem.set(name, { value, expiresAt: Date.now() + ttlS * 1000 });
}

// Synthetic same-zone cache keys; the real GitHub URLs redirect (302 → S3),
// which makes them poor cache keys.
const cacheKey = (name) => new Request(`https://notemd.net/__cache/${name}`);

// Reads honour whatever TTL the write used: the Cache API expires the entry on
// its own Cache-Control, and the memory mirror is refreshed with the remaining
// max-age parsed back off the cached response.
async function cacheReadJson(name) {
  const hit = memGet(name);
  if (hit !== null) return hit;
  const res = await caches.default.match(cacheKey(name));
  if (!res) return null;
  const ttl = Number(res.headers.get("Cache-Control")?.match(/max-age=(\d+)/)?.[1] ?? 60);
  const value = await res.json();
  memPut(name, value, ttl);
  return value;
}

function cacheWriteJson(ctx, name, value, ttlS) {
  memPut(name, value, ttlS);
  const res = new Response(JSON.stringify(value), {
    headers: {
      "Content-Type": "application/json",
      "Cache-Control": `public, max-age=${ttlS}`,
    },
  });
  ctx.waitUntil(caches.default.put(cacheKey(name), res));
}

async function fetchJson(url, name, ttlS, ctx) {
  const cached = await cacheReadJson(name);
  if (cached !== null) return cached;
  const upstream = await fetch(url, {
    redirect: "follow",
    headers: { "User-Agent": UA, Accept: "application/json" },
  });
  if (!upstream.ok) throw new Error(`${name} fetch failed: ${upstream.status}`);
  const value = await upstream.json();
  cacheWriteJson(ctx, name, value, ttlS);
  return value;
}

const fetchManifest = (ctx) => fetchJson(LATEST_JSON_URL, "latest.json", MANIFEST_TTL_S, ctx);
const fetchReleases = (ctx) => fetchJson(RELEASES_API_URL, "releases.json", RELEASES_TTL_S, ctx);

function redirect(location) {
  return new Response(null, {
    status: 302,
    headers: { Location: location, "Cache-Control": "no-store" },
  });
}

// Resolve the installer URL for one (os, arch), with the last known good URL
// as the failure net. Windows packages are built on a second machine *after*
// the macOS release (docs/windows-agent-brief.md), so the newest tag's
// latest.json may carry only darwin-* entries for a while — hence the
// releases-list scan, and hence caring about the last URL that did work.
async function resolveInstaller(target, ctx) {
  const { os, arch } = target;
  const freshKey = `dl/${os}-${arch}`;
  const lkgKey = `dl-lkg/${os}-${arch}`;

  const fresh = await cacheReadJson(freshKey).catch(() => null);
  if (fresh?.url) return fresh.url;

  let url = null;
  try {
    const manifest = await fetchManifest(ctx);
    url = os === "mac" ? macDownloadUrl(manifest, arch) : windowsUrlFromManifest(manifest, arch);
  } catch (e) {
    console.warn("[/download] manifest unavailable:", e);
  }

  if (!url && os === "windows") {
    try {
      const releases = await fetchReleases(ctx);
      // No native arm64 build yet; Windows on ARM runs the x64 installer under
      // emulation, so fall through rather than dead-ending ARM visitors.
      url = windowsUrlFromReleases(releases, arch) ?? windowsUrlFromReleases(releases, "x86_64");
    } catch (e) {
      console.warn("[/download] releases list unavailable:", e);
    }
  }

  if (url) {
    cacheWriteJson(ctx, freshKey, { url }, RESOLVED_TTL_S);
    cacheWriteJson(ctx, lkgKey, { url }, LKG_TTL_S);
    return url;
  }

  // Serving a slightly older installer that certainly exists beats dropping a
  // download click onto the releases list page.
  const lkg = await cacheReadJson(lkgKey).catch(() => null);
  return lkg?.url ?? null;
}

// GET /download[?os=mac|windows][&arch=aarch64|x86_64] → 302 to the installer.
//
// Precedence: explicit ?os=/?arch= > User-Agent / Sec-CH-UA-Arch client hint
// (Chromium only) > per-OS default. Headers cannot reliably distinguish Intel
// from Apple Silicon (Safari reports Intel on both), so the homepage carries
// an explicit Intel fallback link instead. Visitors we can't serve a binary to
// (Linux, mobile, crawlers) get the releases page rather than a download that
// won't run.
async function handleDownload(request, ctx) {
  const url = new URL(request.url);
  const target = detectTarget({
    ua: request.headers.get("User-Agent") || "",
    osParam: url.searchParams.get("os"),
    archParam: url.searchParams.get("arch"),
    archHint: request.headers.get("Sec-CH-UA-Arch"),
  });
  if (!target) return redirect(RELEASES_PAGE);

  try {
    return redirect((await resolveInstaller(target, ctx)) ?? RELEASES_PAGE);
  } catch (e) {
    // Never dead-end a download click; the releases page always works.
    console.warn("[/download] falling back to releases page:", e);
    return redirect(RELEASES_PAGE);
  }
}

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    // /download resolves before the HTTPS upgrade: its target is already an
    // https URL, so redirecting straight there saves a hop (and keeps the
    // route testable under `wrangler dev`, which presents requests as http).
    if (url.pathname === "/download" || url.pathname === "/download/") {
      return handleDownload(request, ctx);
    }
    if (url.protocol === "http:") {
      url.protocol = "https:";
      return Response.redirect(url.toString(), 301);
    }
    return env.ASSETS.fetch(request);
  },
};
