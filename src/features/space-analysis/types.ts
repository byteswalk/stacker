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
