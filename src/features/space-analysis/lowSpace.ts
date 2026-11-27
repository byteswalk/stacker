import { invoke } from "../../invoke";
import { cleanupLabel } from "./components/CleanupResultModal";
import { KNOWN_ITEM_NAMES } from "./knownNames";

export type LowSpaceStep = "idleBuild" | "build" | "caches" | "system";

export type LowSpaceItem = { path: string; label: string; freed: number; error: string | null };
export type LowSpaceStepRecord = { step: LowSpaceStep; freed: number; items: LowSpaceItem[]; more: number };
export type LowSpaceRun = {
  drive: string;
  at: number;
  finishedAt: number;
  total: number;
  freeBefore: number;
  freeAfter: number;
  freed: number;
  manual: boolean;
  steps: LowSpaceStepRecord[];
  notes: string[];
};
export type LowSpaceProgress = { drive: string; step: LowSpaceStep | null };
export type DiskSpace = { root: string; total: number; free: number };
export type LowSpaceStatus = {
  enabled: boolean; thresholdPercent: number; running: LowSpaceProgress | null; runs: LowSpaceRun[]; disks: DiskSpace[];
};
export type LowSpaceCheck = { cleaning: string[]; disks: DiskSpace[] };

/** The order space is freed in, as the page says it. */
export const STEP_LABEL: Record<LowSpaceStep, string> = {
  idleBuild: "闲置项目的开发产物",
  build: "在用项目的开发产物",
  caches: "缓存与下载",
  system: "临时文件与系统缓存",
};

export const lowSpaceStatus = () => invoke<LowSpaceStatus>("low_space_status");
export const saveLowSpace = (enabled: boolean, thresholdPercent: number) =>
  invoke<LowSpaceStatus>("low_space_settings_set", { update: { enabled, thresholdPercent } });
export const checkLowSpaceNow = () => invoke<LowSpaceCheck>("low_space_check_now");

/** A disk as people say it: `D:` rather than `D:\`. */
export const driveName = (root: string) => root.replace(/[\\/]+$/, "") || "/";

/** What an item was, in the page's words: a known cache by its name, anything else as written. */
export const itemLabel = (label: string) => KNOWN_ITEM_NAMES[label] ?? label;

export const reasonLabel = (key: string) => cleanupLabel(key);

/** The threshold the page accepts: a whole percent from 1 to 50. */
export function clampThreshold(value: number): number {
  if (!Number.isFinite(value)) return 5;
  return Math.min(50, Math.max(1, Math.round(value)));
}

/** The newest run on a disk; runs are kept newest first. */
export function lastRunOf(runs: LowSpaceRun[], root: string): LowSpaceRun | undefined {
  const name = driveName(root).toLowerCase();
  return runs.find((run) => driveName(run.drive).toLowerCase() === name);
}

/** A disk's free share, one decimal: 0.5 rather than 0. */
export function freePercent(disk: DiskSpace): string {
  return disk.total > 0 ? (disk.free * 100 / disk.total).toFixed(1) : "0";
}

/** Whether a run left the disk still at or under the threshold. */
export function stillLow(run: Pick<LowSpaceRun, "freeAfter" | "total">, thresholdPercent: number): boolean {
  return run.total > 0 && run.freeAfter * 100 <= run.total * thresholdPercent;
}
