import type { ScanMode } from "./types";

export const LAST_DIRECTORIES_KEY = "stacker.space.lastDirectories";
export const LAST_DRIVES_KEY = "stacker.space.lastDrives";

export type RememberedTargetKind = Extract<ScanMode, "directories" | "drives">;

export interface RememberedTarget {
  target: string;
  valid: boolean;
}

function storageKey(kind: RememberedTargetKind): string {
  return kind === "directories" ? LAST_DIRECTORIES_KEY : LAST_DRIVES_KEY;
}

function comparableTarget(target: string): string {
  return target.trim().replaceAll("/", "\\").toLocaleLowerCase("en-US");
}

function cleanTargets(values: unknown[]): string[] {
  const targets: string[] = [];
  const seen = new Set<string>();
  for (const value of values) {
    if (typeof value !== "string") continue;
    const target = value.trim();
    const comparable = comparableTarget(target);
    if (!target || seen.has(comparable)) continue;
    seen.add(comparable);
    targets.push(target);
  }
  return targets;
}

export function loadRememberedTargets(
  kind: RememberedTargetKind,
  storage: Storage = localStorage,
): string[] {
  const raw = storage.getItem(storageKey(kind));
  if (raw === null) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? cleanTargets(parsed) : [];
  } catch {
    return [];
  }
}

export function markTargetAvailability(
  targets: readonly string[],
  availableTargets: readonly string[],
): RememberedTarget[] {
  const available = new Set(availableTargets.map(comparableTarget));
  return cleanTargets([...targets]).map((target) => ({
    target,
    valid: available.has(comparableTarget(target)),
  }));
}

export function clearRememberedTargets(storage: Storage = localStorage): void {
  storage.removeItem(LAST_DIRECTORIES_KEY);
  storage.removeItem(LAST_DRIVES_KEY);
}

export function applyRememberScanTargetsPreference(
  enabled: boolean,
  storage: Storage = localStorage,
): void {
  if (!enabled) clearRememberedTargets(storage);
}

/** Call only after the backend has accepted and started the scan. */
export function rememberStartedScan(
  mode: ScanMode,
  targets: readonly string[],
  rememberScanTargets: boolean,
  storage: Storage = localStorage,
): void {
  if (!rememberScanTargets) {
    clearRememberedTargets(storage);
    return;
  }
  if (mode === "quick") return;
  storage.setItem(storageKey(mode), JSON.stringify(cleanTargets([...targets])));
}
