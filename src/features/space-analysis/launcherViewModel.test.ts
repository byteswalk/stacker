import { describe, expect, it, vi } from "vitest";
import type { ScanRequest, VolumeInfo } from "./types";
import {
  createDiskSelectorState,
  scanHeaderLayoutClass,
  startAndRememberScan,
} from "./launcherViewModel";

const volumes: VolumeInfo[] = [
  {
    root: "C:\\",
    label: "System",
    fileSystem: "NTFS",
    totalBytes: 512 * 1024 ** 3,
    freeBytes: 128 * 1024 ** 3,
    fixed: true,
  },
  {
    root: "D:\\",
    label: "Work",
    fileSystem: "NTFS",
    totalBytes: 1024 * 1024 ** 3,
    freeBytes: 640 * 1024 ** 3,
    fixed: true,
  },
  {
    root: "Z:\\",
    label: "Network",
    fileSystem: "NTFS",
    totalBytes: 0,
    freeBytes: 0,
    fixed: false,
  },
];

describe("space scan launcher view model", () => {
  it("always opens all-disk analysis with an empty selection", () => {
    expect(createDiskSelectorState("all", volumes, ["C:\\", "D:\\"])).toMatchObject({
      selected: [],
      canStart: false,
      autoStart: false,
    });
  });

  it("shows remembered fixed drives without starting a scan", () => {
    expect(createDiskSelectorState("drives", volumes, ["D:\\", "Z:\\"])).toMatchObject({
      selected: ["D:\\"],
      canStart: true,
      autoStart: false,
    });
  });

  it("never exposes non-fixed volume rows", () => {
    const state = createDiskSelectorState("drives", volumes, []);

    expect(state.volumes.map((volume) => volume.root)).toEqual(["C:\\", "D:\\"]);
  });

  it("uses the same header layout class for idle, running, and completed", () => {
    expect(scanHeaderLayoutClass("idle")).toBe(scanHeaderLayoutClass("running"));
    expect(scanHeaderLayoutClass("running")).toBe(scanHeaderLayoutClass("completed"));
  });

  it("remembers targets only after the backend accepts the scan", async () => {
    const calls: string[] = [];
    const request: ScanRequest = { mode: "directories", targets: ["C:\\work"] };

    const result = await startAndRememberScan(request, true, {
      start: async () => {
        calls.push("start");
        return "scan-42";
      },
      remember: () => {
        calls.push("remember");
        return { ok: true };
      },
    });

    expect(calls).toEqual(["start", "remember"]);
    expect(result).toEqual({ taskId: "scan-42", memory: { ok: true } });
  });

  it("does not remember targets when the backend rejects the scan", async () => {
    const remember = vi.fn(() => ({ ok: true as const }));
    const request: ScanRequest = { mode: "drives", targets: ["D:\\"] };

    await expect(startAndRememberScan(request, true, {
      start: async () => {
        throw new Error("duplicate scan");
      },
      remember,
    })).rejects.toThrow("duplicate scan");
    expect(remember).not.toHaveBeenCalled();
  });
});
