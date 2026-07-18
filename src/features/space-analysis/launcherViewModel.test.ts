import { describe, expect, it, vi } from "vitest";
import type { ScanRequest, VolumeInfo } from "./types";
import {
  beginDiskSelectorRequest,
  closeDiskSelectorRequest,
  createDiskSelectorState,
  diskSelectorResponseIsCurrent,
  launcherControlsDisabled,
  rememberSettingFrom,
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
  {
    root: "E:\\",
    label: "Removable",
    fileSystem: "exFAT",
    totalBytes: 64 * 1024 ** 3,
    freeBytes: 32 * 1024 ** 3,
    fixed: false,
  },
  {
    root: "F:\\",
    label: "Optical",
    fileSystem: "UDF",
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

    expect(state.rows.map((row) => row.root)).toEqual(["C:\\", "D:\\"]);
  });

  it("shows absent remembered drives as invalid but omits known non-fixed roots", () => {
    const state = createDiskSelectorState(
      "drives",
      volumes,
      ["D:\\", "Y:\\", "Z:\\", "E:\\", "F:\\"],
    );

    expect(state.rows.map((row) => ({ root: row.root, available: row.available }))).toEqual([
      { root: "C:\\", available: true },
      { root: "D:\\", available: true },
      { root: "Y:\\", available: false },
    ]);
    expect(state.selected).toEqual(["D:\\"]);
  });

  it("invalidates overlapping and closed disk selector requests", () => {
    const chooseDisk = beginDiskSelectorRequest(0, "drives");
    const allDisk = beginDiskSelectorRequest(chooseDisk.generation, "all");

    expect(diskSelectorResponseIsCurrent(allDisk, chooseDisk)).toBe(false);
    expect(diskSelectorResponseIsCurrent(allDisk, allDisk)).toBe(true);

    const closed = closeDiskSelectorRequest(allDisk.generation);
    const reopened = beginDiskSelectorRequest(closed.generation, "all");
    expect(diskSelectorResponseIsCurrent(closed, allDisk)).toBe(false);
    expect(diskSelectorResponseIsCurrent(reopened, allDisk)).toBe(false);
    expect(reopened.kind).toBe("all");
  });

  it("disables launchers until settings are loaded", () => {
    expect(launcherControlsDisabled({ settings: null, externallyDisabled: false, busy: false, scanActive: false })).toBe(true);
    expect(launcherControlsDisabled({ settings: true, externallyDisabled: false, busy: false, scanActive: false })).toBe(false);
  });

  it("keeps missing or malformed remember settings unknown", () => {
    expect(rememberSettingFrom(undefined)).toBeNull();
    expect(rememberSettingFrom("false")).toBeNull();
    expect(rememberSettingFrom(false)).toBe(false);
    expect(rememberSettingFrom(true)).toBe(true);
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
        return {
          taskId: "scan-42",
          request: { mode: "directories", targets: ["D:\\accepted"] },
          persistenceOwner: true,
        };
      },
      remember: (acceptedRequest) => {
        calls.push("remember");
        expect(acceptedRequest.targets).toEqual(["D:\\accepted"]);
        return { ok: true };
      },
    });

    expect(calls).toEqual(["start", "remember"]);
    expect(result).toEqual({
      taskId: "scan-42",
      request: { mode: "directories", targets: ["D:\\accepted"] },
      memory: { ok: true },
    });
  });

  it("does not persist from a coalesced non-owner start", async () => {
    const remember = vi.fn(() => ({ ok: true as const }));

    const result = await startAndRememberScan(
      { mode: "directories", targets: ["C:\\work"] },
      true,
      {
        start: async () => ({
          taskId: "scan-42",
          request: { mode: "directories", targets: ["C:\\work"] },
          persistenceOwner: false,
        }),
        remember,
      },
    );

    expect(result.memory).toBeNull();
    expect(remember).not.toHaveBeenCalled();
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
