import type { SpaceScanSnapshot } from "./store";

export type QuickScanPhase =
  | "idle"
  | "running"
  | "cancelling"
  | "completed"
  | "cancelled"
  | "failed";

export interface QuickScanView {
  phase: QuickScanPhase;
  title: string;
  description: string;
  primaryLabel: string;
  autoStart: false;
  canStart: boolean;
  canCancel: boolean;
  showProgress: boolean;
  snapshotComparable: boolean;
  errorSummary: string | null;
}

const VIEWS: Record<QuickScanPhase, Omit<QuickScanView, "showProgress" | "snapshotComparable">> = {
  idle: {
    phase: "idle",
    title: "空间分析",
    description: "快速扫描会统计已知开发工具缓存、历史版本和 Windows 临时目录。",
    primaryLabel: "开始扫描",
    autoStart: false,
    canStart: true,
    canCancel: false,
    errorSummary: null,
  },
  running: {
    phase: "running",
    title: "正在快速扫描",
    description: "正在统计已知可清理项，切换页面不会中断扫描。",
    primaryLabel: "取消扫描",
    autoStart: false,
    canStart: false,
    canCancel: true,
    errorSummary: null,
  },
  cancelling: {
    phase: "cancelling",
    title: "正在取消扫描",
    description: "正在停止后台任务，已统计的进度会保留。",
    primaryLabel: "正在取消…",
    autoStart: false,
    canStart: false,
    canCancel: false,
    errorSummary: null,
  },
  completed: {
    phase: "completed",
    title: "扫描完成",
    description: "默认勾选可安全清理项；其他项目需要手动确认。",
    primaryLabel: "重新扫描",
    autoStart: false,
    canStart: true,
    canCancel: false,
    errorSummary: null,
  },
  cancelled: {
    phase: "cancelled",
    title: "扫描已取消，结果不完整",
    description: "已统计的进度已保留，不完整结果不能用于快照比较。",
    primaryLabel: "重新扫描",
    autoStart: false,
    canStart: true,
    canCancel: false,
    errorSummary: null,
  },
  failed: {
    phase: "failed",
    title: "扫描失败",
    description: "页面仍可继续使用，请重试扫描。",
    primaryLabel: "重试",
    autoStart: false,
    canStart: true,
    canCancel: false,
    errorSummary: "扫描任务未完成，详细错误已记录。",
  },
};

function phaseOf(snapshot: SpaceScanSnapshot): QuickScanPhase {
  const state = snapshot.progress?.state;
  if (state === "queued" || state === "running") return "running";
  if (state === "cancelling") return "cancelling";
  if (state === "completed") return "completed";
  if (state === "cancelled") return "cancelled";
  if (state === "failed" || snapshot.error) return "failed";
  if (snapshot.taskId) return "running";
  return "idle";
}

export function quickScanView(snapshot: SpaceScanSnapshot): QuickScanView {
  const phase = phaseOf(snapshot);
  return {
    ...VIEWS[phase],
    showProgress: snapshot.progress !== null,
    snapshotComparable: phase === "completed" && snapshot.result?.completed === true,
  };
}
