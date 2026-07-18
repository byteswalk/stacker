export type ScanMode = "quick" | "directories" | "drives";

export interface ScanRequest {
  mode: ScanMode;
  targets: string[];
}

export interface VolumeInfo {
  root: string;
  label: string;
  fileSystem: string;
  totalBytes: number;
  freeBytes: number;
  fixed: boolean;
}

export type ScanTaskState =
  | "queued"
  | "running"
  | "cancelling"
  | "completed"
  | "cancelled"
  | "failed";

export interface ScanProgress {
  taskId: string;
  state: ScanTaskState;
  scannedFiles: number;
  scannedDirectories: number;
  accountedBytes: number;
  skippedPaths: number;
  elapsedMs: number;
  currentPath: string | null;
}

export interface ScanErrorSummary {
  accessDenied: number;
  vanished: number;
  invalidTarget: number;
  other: number;
}

export interface KnownSpaceItem {
  id: string;
  nameKey: string;
  path: string;
  bytes: number;
  safety: string;
  cleanupKind: string;
  ecosystem: string | null;
}

export interface QuickScanResult {
  taskId: string;
  completed: boolean;
  totalBytes: number;
  safelyReleasableBytes: number;
  items: KnownSpaceItem[];
  errors: ScanErrorSummary;
}

export interface AnalysisSummary {
  taskId: string;
  targets: string[];
  allocatedBytes: number;
  logicalBytes: number;
  fileCount: number;
  directoryCount: number;
  skippedPaths: number;
  rootNodes: DirectoryNode[];
}

export interface DirectoryNode {
  nodeId: string;
  parentId: string | null;
  name: string;
  path: string;
  allocatedBytes: number;
  logicalBytes: number;
  childCount: number;
  safety: string;
  projectId: string | null;
  impactKey: string | null;
  cleanupKind: string | null;
}

export interface LargeFileRow {
  nodeId: string;
  name: string;
  path: string;
  allocatedBytes: number;
  logicalBytes: number;
  modifiedAt: string | null;
}

export interface Paged<T> {
  items: T[];
  offset: number;
  limit: number;
  total: number;
}

export type SafetyClass = "safe" | "rebuildable" | "needsConfirmation" | "viewOnly";
export type ElevationRequirement = "none" | "required";

export interface CleanupPlanItem {
  nodeId: string;
  path: string;
  estimatedBytes: number;
  safety: SafetyClass;
  impactKey: string;
  cleanupKind: string;
  requiresElevation: boolean;
  defaultSelected: boolean;
}

export interface CleanupPlan {
  planId: string;
  scanTaskId: string;
  createdAt: string;
  estimatedBytes: number;
  elevationRequirement: ElevationRequirement;
  items: CleanupPlanItem[];
}

export type CleanupTaskState = "queued" | "running" | "cancelling" | "completed" | "cancelled" | "failed";
export type CleanupItemState = "pending" | "running" | "completed" | "skipped" | "failed" | "cancelled";

export interface CleanupProgress {
  taskId: string;
  planId: string;
  state: CleanupTaskState;
  completedItems: number;
  totalItems: number;
  actualReleasedBytes: number;
  currentNodeId: string | null;
}

export interface CleanupItemResult {
  nodeId: string;
  path: string;
  state: CleanupItemState;
  validatedBytes: number;
  actualReleasedBytes: number;
  reasonKey: string | null;
}

export interface CleanupResult {
  taskId: string;
  planId: string;
  state: CleanupTaskState;
  actualReleasedBytes: number;
  items: CleanupItemResult[];
}
