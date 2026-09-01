// Pure resolution logic behind notemd.net/download — no I/O, no globals.
// The worker (src/index.js) does the fetching and caching; everything that
// decides *which* file a visitor should get lives here so it can be unit
// tested. Mirrors the scripts/merge-latest-json-core.mjs split.

export const GH_REPO = "wizlijun/note.md";
export const RELEASES_PAGE = `https://github.com/${GH_REPO}/releases`;

// Sec-CH-UA-Arch arrives quoted ("arm"); query params come from us or from
// users hand-editing URLs, so accept the common spellings.
export function normalizeArch(raw) {
  if (!raw) return null;
  const v = raw.toLowerCase().replace(/"/g, "").trim();
  if (v === "aarch64" || v === "arm64" || v === "arm") return "aarch64";
  if (v === "x86_64" || v === "x64" || v === "x86" || v === "amd64" || v === "intel") return "x86_64";
  return null;
}

export function normalizeOs(raw) {
  if (!raw) return null;
  const v = raw.toLowerCase().trim();
  if (v === "windows" || v === "win" || v === "win32" || v === "win64") return "windows";
  if (v === "mac" || v === "macos" || v === "osx" || v === "darwin") return "mac";
  return null;
}

// UA sniffing is deliberately narrow: we only need to tell "this machine can
// run a .dmg" from "this machine can run a .exe". Anything else (Linux, iOS,
// Android, crawlers) gets no binary — the caller sends it to the releases page.
export function osFromUserAgent(ua) {
  if (!ua) return null;
  if (/iPhone|iPad|iPod|Android/i.test(ua)) return null;
  if (/Macintosh|Mac OS X/.test(ua)) return "mac";
  if (/Windows NT/.test(ua)) return "windows";
  return null;
}

/**
 * Decide what to hand this visitor.
 * @returns {{os: 'mac'|'windows', arch: 'aarch64'|'x86_64'}|null}
 *
 * Precedence: explicit ?os= / ?arch= > Sec-CH-UA-Arch client hint (Chromium
 * only) > per-OS default. Headers cannot reliably distinguish Intel from Apple
 * Silicon (Safari reports Intel on both), so the homepage carries an explicit
 * Intel fallback link instead of us guessing harder.
 */
export function detectTarget({ ua = "", osParam = null, archParam = null, archHint = null } = {}) {
  const os = normalizeOs(osParam) ?? osFromUserAgent(ua);
  if (!os) return null;
  const arch = normalizeArch(archParam) ?? normalizeArch(archHint) ?? (os === "mac" ? "aarch64" : "x86_64");
  return { os, arch };
}

// Derive the tag from an updater URL (…/releases/download/<tag>/…) rather than
// assuming v<version>, so a tag-format change can't break us.
export function tagFromUrl(url) {
  return url?.match(/\/releases\/download\/([^/]+)\//)?.[1] ?? null;
}

/**
 * macOS: the updater artifact is a tarball, not the .dmg users want, so the
 * installer filename has to be composed from version + arch.
 */
export function macDownloadUrl(manifest, arch) {
  const version = manifest?.version;
  if (!version) return null;
  const tag = tagFromUrl(manifest?.platforms?.[`darwin-${arch}`]?.url) ?? `v${version}`;
  return `https://github.com/${GH_REPO}/releases/download/${tag}/note.md-${version}-${arch}.dmg`;
}

/**
 * Windows: Tauri 2 signs the NSIS installer itself (there is no separate
 * .nsis.zip like Tauri 1), so the updater URL *is* the setup.exe. Use it
 * verbatim — no filename guessing.
 */
export function windowsUrlFromManifest(manifest, arch) {
  const url = manifest?.platforms?.[`windows-${arch}`]?.url;
  return typeof url === "string" && url.endsWith(".exe") ? url : null;
}

// Tauri NSIS names bundles `<product>_<version>_<arch>-setup.exe`.
const NSIS_ARCH = { aarch64: "arm64", x86_64: "x64" };

export function isWindowsInstaller(name, arch) {
  const suffix = NSIS_ARCH[arch];
  return !!suffix && new RegExp(`_${suffix}-setup\\.exe$`, "i").test(name || "");
}

/**
 * Windows packages are built on a second machine *after* the macOS release
 * (docs/windows-agent-brief.md), so the newest tag's latest.json may carry
 * only darwin-* entries for a while. Walk the releases list and take the most
 * recent one that actually ships an installer for this arch.
 *
 * `releases` is the GitHub /releases payload; it arrives newest-first, and we
 * preserve that order rather than parsing versions out of tag names.
 */
export function windowsUrlFromReleases(releases, arch) {
  for (const rel of Array.isArray(releases) ? releases : []) {
    if (rel?.draft || rel?.prerelease) continue;
    for (const asset of rel?.assets ?? []) {
      if (isWindowsInstaller(asset?.name, arch) && asset?.browser_download_url) {
        return asset.browser_download_url;
      }
    }
  }
  return null;
}

/**
 * GitHub caps the releases endpoint at 100 items per page. Windows builds can
 * trail macOS by an arbitrary number of releases, so keep walking until an
 * installer is found instead of treating the first page as the full history.
 *
 * `loadPage` receives a one-based page number. A short page (including an
 * empty one after an exactly-full final page) marks the end of the list.
 */
export async function windowsUrlFromReleasePages(loadPage, arch, pageSize) {
  for (let page = 1; ; page += 1) {
    const releases = await loadPage(page);
    if (!Array.isArray(releases)) return null;

    const url = windowsUrlFromReleases(releases, arch);
    if (url) return url;
    if (releases.length < pageSize) return null;
  }
}
