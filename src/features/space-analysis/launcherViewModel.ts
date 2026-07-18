import type { QuickScanPhase } from "./viewModel";
import type { ScanRequest, VolumeInfo } from "./types";
import type { TargetStorageResult } from "./targetStore";
import type { ScanStartResult } from "./store";

export type DiskSelectorKind = "drives" | "all";

export interface DiskSelectorState {
  rows: DiskSelectorRow[];
  selected: string[];
  canStart: boolean;
  autoStart: false;
}

export interface DiskSelectorRow extends VolumeInfo {
  available: boolean;
  remembered: boolean;
}

export interface DiskSelectorRequestIdentity {
  generation: number;
  kind: DiskSelectorKind | null;
}

export interface StartAndRememberDependencies {
  start: (request: ScanRequest) => Promise<ScanStartResult>;
  remember: (
    request: ScanRequest,
    rememberScanTargets: boolean,
  ) => TargetStorageResult;
}

function comparableRoot(root: string): string {
  return root.trim().replaceAll("/", "\\").toLocaleLowerCase("en-US");
}

export function createDiskSelectorState(
  kind: DiskSelectorKind,
  availableVolumes: readonly VolumeInfo[],
  rememberedTargets: readonly string[],
): DiskSelectorState {
  const volumes = availableVolumes.filter((volume) => volume.fixed);
  const availableRoots = new Map(
    volumes.map((volume) => [comparableRoot(volume.root), volume]),
  );
  const rememberedRoots = new Map(
    rememberedTargets.map((target) => [comparableRoot(target), target.trim()]),
  );
  const selected = kind === "all"
    ? []
    : rememberedTargets.flatMap((target) => {
      const volume = availableRoots.get(comparableRoot(target));
      return volume ? [volume.root] : [];
    });
  const availableRows = volumes.map<DiskSelectorRow>((volume) => ({
    ...volume,
    available: true,
    remembered: kind === "drives" && rememberedRoots.has(comparableRoot(volume.root)),
  }));
  const unavailableRows = kind === "all"
    ? []
    : [...rememberedRoots.entries()].flatMap(([comparable, root]) => (
      availableRoots.has(comparable)
        ? []
        : [{
          root,
          label: "",
          fileSystem: "",
          totalBytes: 0,
          freeBytes: 0,
          fixed: false,
          available: false,
          remembered: true,
        } satisfies DiskSelectorRow]
    ));

  return {
    rows: [...availableRows, ...unavailableRows],
    selected: [...new Set(selected)],
    canStart: selected.length > 0,
    autoStart: false,
  };
}

export function beginDiskSelectorRequest(
  generation: number,
  kind: DiskSelectorKind,
): DiskSelectorRequestIdentity {
  return { generation: generation + 1, kind };
}

export function closeDiskSelectorRequest(generation: number): DiskSelectorRequestIdentity {
  return { generation: generation + 1, kind: null };
}

export function diskSelectorResponseIsCurrent(
  active: DiskSelectorRequestIdentity,
  request: DiskSelectorRequestIdentity,
): boolean {
  return active.kind !== null
    && active.generation === request.generation
    && active.kind === request.kind;
}

export function launcherControlsDisabled({
  settings,
  externallyDisabled,
  busy,
  scanActive,
}: {
  settings: boolean | null;
  externallyDisabled: boolean;
  busy: boolean;
  scanActive: boolean;
}): boolean {
  return settings === null || externallyDisabled || busy || scanActive;
}

export function rememberSettingFrom(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}

export function scanHeaderLayoutClass(phase: QuickScanPhase): string {
  void phase;
  return "clhero space-scan-header";
}

export async function startAndRememberScan(
  request: ScanRequest,
  rememberScanTargets: boolean,
  dependencies: StartAndRememberDependencies,
): Promise<{
  taskId: string;
  request: ScanRequest;
  memory: TargetStorageResult | null;
}> {
  const accepted = await dependencies.start(request);
  const memory = accepted.persistenceOwner
    ? dependencies.remember(accepted.request, rememberScanTargets)
    : null;
  return {
    taskId: accepted.taskId,
    request: accepted.request,
    memory,
  };
}
