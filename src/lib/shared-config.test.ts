import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  readSharedConfig, writeSharedConfig, getGitProxy, setGitProxy, type SharedConfig,
} from "./shared-config";

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
      git_proxy: "http://127.0.0.1:1080",
    };
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await writeSharedConfig(cfg);
    expect(invoke).toHaveBeenCalledWith("shared_config_write", { cfg });
  });

  it("getGitProxy delegates to git_proxy_get", async () => {
    vi.mocked(invoke).mockResolvedValueOnce("http://127.0.0.1:1080");
    expect(await getGitProxy()).toBe("http://127.0.0.1:1080");
    expect(invoke).toHaveBeenCalledWith("git_proxy_get");
  });

  it("setGitProxy passes the value through and returns the normalized form", async () => {
    vi.mocked(invoke).mockResolvedValueOnce("http://127.0.0.1:1080");
    const got = await setGitProxy("  http://127.0.0.1:1080  ");
    expect(invoke).toHaveBeenCalledWith("git_proxy_set", { value: "  http://127.0.0.1:1080  " });
    expect(got).toBe("http://127.0.0.1:1080");
  });

  it("setGitProxy surfaces the host's rejection so the UI can show it verbatim", async () => {
    vi.mocked(invoke).mockRejectedValueOnce("unsupported proxy scheme 'ftp'");
    await expect(setGitProxy("ftp://x")).rejects.toBe("unsupported proxy scheme 'ftp'");
  });
});
