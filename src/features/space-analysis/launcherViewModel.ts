import type { QuickScanPhase } from "./viewModel";
import type { ScanRequest, VolumeInfo } from "./types";
import type { TargetStorageResult } from "./targetStore";

export type DiskSelectorKind = "drives" | "all";

export interface DiskSelectorState {
  volumes: VolumeInfo[];
  selected: string[];
  canStart: boolean;
  autoStart: false;
}

export interface StartAndRememberDependencies {
  start: (request: ScanRequest) => Promise<string>;
  remember: (
    request: ScanRequest,
    rememberScanTargets: boolean,
  ) => TargetStorageResult;
}

export function createDiskSelectorState(
  kind: DiskSelectorKind,
  availableVolumes: readonly VolumeInfo[],
  rememberedTargets: readonly string[],
): DiskSelectorState {
  const volumes = availableVolumes.filter((volume) => volume.fixed);
  const availableRoots = new Map(
    volumes.map((volume) => [volume.root.toLocaleLowerCase("en-US"), volume.root]),
  );
  const selected = kind === "all"
    ? []
    : rememberedTargets.flatMap((target) => {
      const root = availableRoots.get(target.toLocaleLowerCase("en-US"));
      return root ? [root] : [];
    });

  return {
    volumes,
    selected: [...new Set(selected)],
    canStart: selected.length > 0,
    autoStart: false,
  };
}

export function scanHeaderLayoutClass(phase: QuickScanPhase): string {
  void phase;
  return "clhero space-scan-header";
}

export async function startAndRememberScan(
  request: ScanRequest,
  rememberScanTargets: boolean,
  dependencies: StartAndRememberDependencies,
): Promise<{ taskId: string; memory: TargetStorageResult }> {
  const taskId = await dependencies.start(request);
  const memory = dependencies.remember(request, rememberScanTargets);
  return { taskId, memory };
}
