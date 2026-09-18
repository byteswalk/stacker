import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { debug, error, warn } from "@tauri-apps/plugin-log";
import { invoke, reportFrontendError, reportFrontendWarning, safeLogText } from "./invoke";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-log", () => ({ debug: vi.fn(), error: vi.fn(), warn: vi.fn() }));

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(debug).mockResolvedValue();
  vi.mocked(error).mockResolvedValue();
  vi.mocked(warn).mockResolvedValue();
});

describe("safeLogText", () => {
  it("redacts supported access-token formats", () => {
    const text = safeLogText(
      "pt-abcdefghijklmnop github_pat_abcdefghijklmnop authorization=Bearer bearer-secret-value password=hunter2",
    );

    expect(text).not.toContain("pt-abcdefghijklmnop");
    expect(text).not.toContain("github_pat_abcdefghijklmnop");
    expect(text).not.toContain("bearer-secret-value");
    expect(text).not.toContain("hunter2");
  });

  it("redacts credentials embedded in URLs while preserving diagnostics", () => {
    const text = safeLogText(
      "request failed: https://example.test/api?access_token=secret-value&resource=release status=403",
    );

    expect(text).toContain("status=403");
    expect(text).toContain("resource=release");
    expect(text).not.toContain("secret-value");
  });

  it("redacts URL userinfo and Basic authorization", () => {
    const text = safeLogText("https://alice:secret@example.test/path Authorization: Basic c2VjcmV0");
    expect(text).not.toContain("secret");
    expect(text).not.toContain("c2VjcmV0");
    expect(text).toContain("example.test/path");
  });
});

describe("command diagnostics", () => {
  it("does not delay a command or its result when the log channel is stalled", async () => {
    vi.mocked(debug).mockImplementation(() => new Promise(() => {}));
    vi.mocked(tauriInvoke).mockResolvedValue({ updated: true });
    await expect(invoke("test_command")).resolves.toEqual({ updated: true });
    expect(tauriInvoke).toHaveBeenCalledOnce();
  }, 1000);

  it("preserves errors when error logging stalls", async () => {
    vi.mocked(error).mockImplementation(() => new Promise(() => {}));
    const cause = new Error("failure");
    vi.mocked(tauriInvoke).mockRejectedValue(cause);
    await expect(invoke("test_command")).rejects.toBe(cause);
  }, 1000);

  it("redacts both diagnostic messages and their causes", () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    const consoleWarning = vi.spyOn(console, "warn").mockImplementation(() => {});
    reportFrontendError("password=one", "https://user:two@example.test");
    reportFrontendWarning("password=three");
    expect(vi.mocked(error).mock.calls[0][0]).not.toMatch(/one|two/);
    expect(vi.mocked(warn).mock.calls[0][0]).not.toContain("three");
    expect(consoleError).toHaveBeenCalledWith(vi.mocked(error).mock.calls[0][0]);
    expect(consoleWarning).toHaveBeenCalledWith(vi.mocked(warn).mock.calls[0][0]);
    consoleError.mockRestore();
    consoleWarning.mockRestore();
  });
});
