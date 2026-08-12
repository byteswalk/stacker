import { useState } from "react";
import { useI18n } from "../../../i18n";
import { invoke } from "../../../invoke";
import { useToast } from "../../../ui";
import type { AnalysisSummary, SkippedPathEntry } from "../types";

const INITIAL_SKIPPED_PATHS = 50;

function skippedReason(reason: SkippedPathEntry["reason"], tr: (value: string) => string) {
  const labels: Record<SkippedPathEntry["reason"], string> = {
    accessDenied: "访问被拒绝",
    vanished: "扫描期间已消失",
    invalidTarget: "路径无效",
    reparsePoint: "链接或重解析点",
    duplicateFile: "重复文件",
    unsupportedFileType: "不支持的文件类型",
    unreadable: "无法读取",
    other: "其他原因",
  };
  return tr(labels[reason] ?? labels.other);
}

export function SkippedPaths({ summary }: { summary: AnalysisSummary }) {
  const { tr } = useI18n();
  const toast = useToast();
  const [showAll, setShowAll] = useState(false);
  const entries = summary.skippedPathEntries ?? [];
  const listedOccurrences = entries.reduce((total, entry) => total + entry.occurrences, 0);
  const unlisted = summary.unlistedSkippedPaths
    ?? Math.max(0, summary.skippedPaths - listedOccurrences);
  const visibleEntries = showAll ? entries : entries.slice(0, INITIAL_SKIPPED_PATHS);

  async function openDirectory(path: string) {
    try {
      await invoke("space_open_directory", { path });
    } catch {
      toast(tr("无法打开文件夹。请确认路径仍然存在，并且当前账户拥有访问权限。"), "err");
    }
  }

  return (
    <section className="space-skipped-paths standalone" aria-label={tr("已跳过路径")}>
      <div className="space-analysis-section-heading">
        <div>
          <strong>{tr("已跳过路径")}</strong>
          <span>{tr("这些路径未计入空间统计。可查看跳过原因，并打开仍然存在的路径所在目录。")}</span>
        </div>
        <span>
          {tr("已记录")} {entries.length.toLocaleString()}
          {" / "}{tr("共")} {summary.skippedPaths.toLocaleString()}
        </span>
      </div>

      {visibleEntries.length > 0 ? (
        <div className="space-skipped-list">
          {visibleEntries.map((entry) => (
            <div className="space-skipped-row" key={`${entry.reason}:${entry.path}`}>
              <i className="ti ti-folder-x" aria-hidden="true" />
              <div className="space-skipped-main">
                <strong title={entry.path}>{entry.path}</strong>
                <span>{skippedReason(entry.reason, tr)}</span>
              </div>
              {entry.occurrences > 1 && (
                <span className="bd" title={tr("同一路径被多次跳过")}>×{entry.occurrences.toLocaleString()}</span>
              )}
              <button
                type="button"
                className="space-icon-button"
                title={tr("打开所在目录")}
                aria-label={`${tr("打开所在目录")}: ${entry.path}`}
                onClick={() => void openDirectory(entry.path)}
              >
                <i className="ti ti-folder-open" />
              </button>
            </div>
          ))}
          {entries.length > INITIAL_SKIPPED_PATHS && (
            <button
              type="button"
              className="space-load-more"
              onClick={() => setShowAll((current) => !current)}
            >
              <i className={`ti ${showAll ? "ti-chevron-up" : "ti-chevron-down"}`} />
              {showAll
                ? tr("收起跳过路径")
                : `${tr("显示全部")} ${entries.length.toLocaleString()} ${tr("条记录")}`}
            </button>
          )}
        </div>
      ) : (
        <div className="space-analysis-empty">
          <i className="ti ti-circle-check" />
          <span>{summary.skippedPaths > 0
            ? tr("扫描器未能取得这些跳过项的具体路径。")
            : tr("本次扫描没有跳过路径。")}</span>
        </div>
      )}

      {unlisted > 0 && (
        <div className="space-skipped-note">
          <i className="ti ti-info-circle" />
          {tr("为控制内存占用，另有")} {unlisted.toLocaleString()} {tr("条跳过记录未展开。")}
        </div>
      )}
    </section>
  );
}
