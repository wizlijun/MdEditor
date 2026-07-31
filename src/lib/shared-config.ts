import { invoke } from "@tauri-apps/api/core";

export interface SharedConfig {
  version: number;
  sotvault: string | null;
  rawvault: string | null;
  calibre_path: string | null;
}

export async function readSharedConfig(): Promise<SharedConfig> {
  return await invoke("shared_config_read");
}

export async function writeSharedConfig(cfg: SharedConfig): Promise<void> {
  await invoke("shared_config_write", { cfg });
}
