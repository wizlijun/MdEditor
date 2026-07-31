import { describe, it, expect, vi, beforeEach } from "vitest";
import { readSharedConfig, writeSharedConfig, type SharedConfig } from "./shared-config";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
import { invoke } from "@tauri-apps/api/core";

describe("shared-config", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("readSharedConfig delegates to shared_config_read command", async () => {
    const fake = {
      version: 1, sotvault: "/x", rawvault: null, calibre_path: null,
    } satisfies Partial<SharedConfig>;
    vi.mocked(invoke).mockResolvedValueOnce(fake);
    const got = await readSharedConfig();
    expect(invoke).toHaveBeenCalledWith("shared_config_read");
    expect(got).toEqual(fake);
  });

  it("writeSharedConfig delegates to shared_config_write command", async () => {
    const cfg: SharedConfig = {
      version: 1, sotvault: "/x", rawvault: "/y", calibre_path: "/z",
    };
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await writeSharedConfig(cfg);
    expect(invoke).toHaveBeenCalledWith("shared_config_write", { cfg });
  });
});
