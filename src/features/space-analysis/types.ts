export type ScanMode = "quick" | "directories" | "drives";

export interface ScanRequest {
  mode: ScanMode;
  targets: string[];
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
