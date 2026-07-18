import type { ReactNode } from "react";
import { useI18n } from "../../../i18n";
import type { SpaceScanSnapshot } from "../store";
import { quickScanView, type QuickScanPhase } from "../viewModel";
import { scanHeaderLayoutClass } from "../launcherViewModel";

type Translate = (value: string) => string;

function formatBytes(bytes: number) {
  if (bytes >= 1024 ** 4) return `${(bytes / 1024 ** 4).toFixed(1)} TB`;
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(0)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${bytes} B`;
}

function targetSummary(scan: SpaceScanSnapshot, tr: Translate) {
  if (!scan.request) return tr("尚未选择扫描目标");
  if (scan.request.mode === "quick") return tr("快速扫描 · 已知开发工具缓存与临时目录");
  const prefix = scan.request.mode === "directories" ? tr("目录") : tr("磁盘");
  return `${prefix} · ${scan.request.targets.join(" · ")}`;
}

function deepScanCopy(phase: QuickScanPhase, tr: Translate) {
  if (phase === "running") return {
    title: tr("正在分析所选范围"),
    description: tr("正在统计文件的实际分配空间。切换页面不会中断扫描。"),
  };
  if (phase === "cancelling") return {
    title: tr("正在取消扫描"),
    description: tr("正在停止后台任务，已统计的进度会保留。"),
  };
  if (phase === "completed") return {
    title: tr("空间分析完成"),
    description: tr("所选范围已完成统计，可继续选择其他扫描范围。"),
  };
  if (phase === "cancelled") return {
    title: tr("扫描已取消，结果不完整"),
    description: tr("已统计的进度已保留，不完整结果不能用于快照比较。"),
  };
  if (phase === "failed") return {
    title: tr("扫描失败"),
    description: tr("页面仍可继续使用，请重新选择扫描范围。"),
  };
  return { title: tr("空间分析"), description: tr("请选择扫描范围后手动开始分析。") };
}

export function ScanHeader({
  scan,
  headline,
  idleDescription,
  actionBusy = false,
  trailingAction,
  onCancel,
}: {
  scan: SpaceScanSnapshot;
  headline?: string;
  idleDescription?: string;
  actionBusy?: boolean;
  trailingAction?: ReactNode;
  onCancel: () => void;
}) {
  const { locale, tr } = useI18n();
  const view = quickScanView(scan);
  const progress = scan.progress;
  const active = view.phase === "running" || view.phase === "cancelling";
  const isDeep = scan.request?.mode === "directories" || scan.request?.mode === "drives";
  const copy = isDeep
    ? deepScanCopy(view.phase, tr)
    : { title: tr(view.title), description: view.phase === "idle" && idleDescription ? idleDescription : tr(view.description) };
  const currentPath = progress?.currentPath || (active ? tr("等待扫描进度…") : tr("等待手动开始扫描"));
  const number = new Intl.NumberFormat(locale);
  const metrics = [
    [tr("已扫描文件"), number.format(progress?.scannedFiles ?? 0)],
    [tr("已扫描目录"), number.format(progress?.scannedDirectories ?? 0)],
    [tr("已统计分配空间"), formatBytes(progress?.accountedBytes ?? 0)],
    [tr("耗时"), `${number.format((progress?.elapsedMs ?? 0) / 1000)} s`],
    [tr("已跳过"), number.format(progress?.skippedPaths ?? 0)],
  ];

  return (
    <section
      className={scanHeaderLayoutClass(view.phase)}
      data-active={active ? "true" : "false"}
      data-phase={view.phase}
    >
      {active && <span className="border-runner" aria-hidden="true" />}
      <span className="clic"><i className={`ti ${active ? "ti-database-search" : "ti-database"}`} /></span>
      <div className="scan-header-summary">
        <div className="scan-header-title" title={headline ?? copy.title}>{headline ?? copy.title}</div>
        <div className="scan-header-description">{copy.description}</div>
        <div className="scan-target-summary" title={targetSummary(scan, tr)}>
          <i className="ti ti-target-arrow" aria-hidden="true" />
          <span>{targetSummary(scan, tr)}</span>
        </div>
        {view.errorSummary && (
          <div className="scan-error-summary" title={scan.taskId ? `${tr("任务 ID")}: ${scan.taskId}` : undefined}>
            <i className="ti ti-alert-triangle" /> {tr(view.errorSummary)}
          </div>
        )}
      </div>
      <div className="scan-header-progress" aria-live="polite">
        <div className="scan-metrics">
          {metrics.map(([label, value]) => (
            <div className="scan-metric" key={label} title={`${label}: ${value}`}>
              <span>{label}</span>
              <b>{value}</b>
            </div>
          ))}
        </div>
        <div className="scan-current-path" title={currentPath}>
          <i className="ti ti-folder-search" aria-hidden="true" />
          <span>{currentPath}</span>
        </div>
      </div>
      <div className="scan-header-actions">
        {active && (
          <button className="gh" disabled={!view.canCancel || actionBusy} onClick={onCancel}>
            <i className={`ti ${view.phase === "cancelling" || actionBusy ? "ti-loader spin" : "ti-x"}`} />
            {tr(view.primaryLabel)}
          </button>
        )}
        {trailingAction}
      </div>
    </section>
  );
}
