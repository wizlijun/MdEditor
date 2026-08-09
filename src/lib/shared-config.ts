import { invoke } from "@tauri-apps/api/core";

export interface SharedConfig {
  version: number;
  sotvault: string | null;
  rawvault: string | null;
  calibre_path: string | null;
  /** Proxy for vault sync's `git` subprocess. Machine-local; never synced. */
  git_proxy: string | null;
}

export async function readSharedConfig(): Promise<SharedConfig> {
  return await invoke("shared_config_read");
}

export async function writeSharedConfig(cfg: SharedConfig): Promise<void> {
  await invoke("shared_config_write", { cfg });
}

/** The proxy vault sync's `git` runs through; `""` when unset. */
export async function getGitProxy(): Promise<string> {
  return await invoke("git_proxy_get");
}

/**
 * Set (or clear, with a blank value) that proxy. Returns the stored,
 * normalized value.
 *
 * Goes through its own command rather than `writeSharedConfig` so the host
 * validates the URL and preserves the rest of shared.json — that file also
 * carries the vault path, and other processes write it too.
 *
 * Rejects with the host's explanation ("unsupported proxy scheme 'ftp'", …),
 * which is meant to be shown verbatim.
 */
export async function setGitProxy(value: string): Promise<string> {
  return await invoke("git_proxy_set", { value });
}
