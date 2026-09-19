import { afterEach, describe, expect, it, vi } from "vitest";
import { saveWith, type DownloadsApi } from "./download";

type Listener = (d: chrome.downloads.DownloadDelta) => void;

function fakeDownloads(onStart: (id: number, fire: Listener) => void = () => {}) {
  const listeners = new Set<Listener>();
  const fire: Listener = (d) => listeners.forEach((l) => l(d));
  const api = {
    download: vi.fn(async () => { onStart(7, fire); return 7; }),
    onChanged: {
      addListener: vi.fn((l: Listener) => listeners.add(l)),
      removeListener: vi.fn((l: Listener) => listeners.delete(l)),
    },
  };
  return { api: api as unknown as DownloadsApi, listeners, fire };
}

afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

describe("saveWith", () => {
  it("resolves only once the download completes, then cleans up", async () => {
    const revoke = vi.spyOn(URL, "revokeObjectURL");
    const { api, listeners, fire } = fakeDownloads();
    let settled = false;
    const p = saveWith(api, "a.md", "hi", "text/markdown").then(() => { settled = true; });
    await Promise.resolve(); await Promise.resolve();
    fire({ id: 99, state: { current: "complete" } });
    fire({ id: 7, state: { current: "in_progress" } });
    await Promise.resolve();
    expect(settled).toBe(false);
    fire({ id: 7, state: { current: "complete" } });
    await p;
    expect(settled).toBe(true);
    expect(listeners.size).toBe(0);
    expect(revoke).toHaveBeenCalledTimes(1);
    expect((api.download as ReturnType<typeof vi.fn>).mock.calls[0][0]).toMatchObject({ filename: "Stacker 网页对话/a.md", saveAs: false });
  });
  it("handles a completion reported before download() returns", async () => {
    const { api, listeners } = fakeDownloads((id, fire) => fire({ id, state: { current: "complete" } }));
    await expect(saveWith(api, "a.md", "hi", "text/markdown")).resolves.toBeUndefined();
    expect(listeners.size).toBe(0);
  });
  it("rejects with the reason when the download is interrupted", async () => {
    const revoke = vi.spyOn(URL, "revokeObjectURL");
    const { api, listeners } = fakeDownloads((id, fire) => queueMicrotask(() => fire({ id, state: { current: "interrupted" }, error: { current: "FILE_NO_SPACE" } })));
    await expect(saveWith(api, "a.md", "hi", "text/markdown")).rejects.toThrow("download interrupted: FILE_NO_SPACE");
    expect(listeners.size).toBe(0);
    expect(revoke).toHaveBeenCalledTimes(1);
  });
  it("rejects when the download never finishes", async () => {
    vi.useFakeTimers();
    const { api, listeners } = fakeDownloads();
    const p = saveWith(api, "a.md", "hi", "text/markdown", 1000);
    const check = expect(p).rejects.toThrow("download timed out");
    await vi.advanceTimersByTimeAsync(1000);
    await check;
    expect(listeners.size).toBe(0);
  });
});
