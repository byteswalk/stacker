import type { ScanMode } from "./types";

export const LAST_DIRECTORIES_KEY = "stacker.space.lastDirectories";
export const LAST_DRIVES_KEY = "stacker.space.lastDrives";

export type RememberedTargetKind = Extract<ScanMode, "directories" | "drives">;

export interface RememberedTarget {
  target: string;
  valid: boolean;
}

export type TargetStorageResult =
  | { ok: true }
  | { ok: false; error: unknown };

export type DisableRememberScanTargetsResult =
  | { ok: true }
  | { ok: false; stage: "settings" | "storage"; error: unknown };

const STORAGE_UNAVAILABLE = "Browser storage is unavailable";

function resolveStorage(storage?: Storage | null): Storage | null {
  if (storage !== undefined) return storage;
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
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
  storage?: Storage | null,
): string[] {
  try {
    const targetStorage = resolveStorage(storage);
    if (!targetStorage) return [];
    const raw = targetStorage.getItem(storageKey(kind));
    if (raw === null) return [];
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

export function clearRememberedTargets(storage?: Storage | null): TargetStorageResult {
  const targetStorage = resolveStorage(storage);
  if (!targetStorage) return { ok: false, error: new Error(STORAGE_UNAVAILABLE) };

  let firstError: unknown;
  for (const key of [LAST_DIRECTORIES_KEY, LAST_DRIVES_KEY]) {
    try {
      targetStorage.removeItem(key);
    } catch (error) {
      firstError ??= error;
    }
  }
  return firstError === undefined ? { ok: true } : { ok: false, error: firstError };
}

export async function disableRememberScanTargets(
  saveSettings: () => Promise<unknown>,
  storage?: Storage | null,
): Promise<DisableRememberScanTargetsResult> {
  try {
    await saveSettings();
  } catch (error) {
    return { ok: false, stage: "settings", error };
  }

  const cleared = clearRememberedTargets(storage);
  return cleared.ok
    ? cleared
    : { ok: false, stage: "storage", error: cleared.error };
}

/** Call only after the backend has accepted and started the scan. */
export function rememberStartedScan(
  mode: ScanMode,
  targets: readonly string[],
  rememberScanTargets: boolean,
  storage?: Storage | null,
): TargetStorageResult {
  if (!rememberScanTargets || mode === "quick") return { ok: true };
  const targetStorage = resolveStorage(storage);
  if (!targetStorage) return { ok: false, error: new Error(STORAGE_UNAVAILABLE) };
  try {
    targetStorage.setItem(storageKey(mode), JSON.stringify(cleanTargets([...targets])));
    return { ok: true };
  } catch (error) {
    return { ok: false, error };
  }
}
