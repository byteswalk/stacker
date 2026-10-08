import { useState } from "react";
import { useI18n } from "../../../i18n";
import { invoke } from "../../../invoke";
import { useToast } from "../../../ui";
import type { AnalysisSummary, SkippedPathEntry } from "../types";
import { AiAskModal, askAi } from "../../ai/AiAsk";

const INITIAL_SKIPPED_PATHS = 50;

/** Why the scanner left a path out, and what that means for the totals. */
const REASONS: Record<SkippedPathEntry["reason"], [string, string]> = {
  accessDenied: ["访问被拒绝", "当前账户没有权限读取，以管理员身份扫描可以计入"],
  vanished: ["扫描期间已消失", "扫描时文件被删除或移动了，多为临时文件"],
  invalidTarget: ["路径无效", "路径格式或长度不受支持"],
  reparsePoint: ["链接或目录联接", "指向别处的符号链接、目录联接或云盘占位；目标本身另行统计，这里不重复计入"],
  duplicateFile: ["硬链接（已在别处计入）", "与另一个路径是磁盘上的同一份文件（pnpm、Windows 组件常这样共享），只占一份空间，已在别处统计"],
  unsupportedFileType: ["不支持的文件类型", "设备文件、管道等不占普通磁盘空间的项目"],
  unreadable: ["无法读取", "文件被占用或已损坏，读不到大小"],
  other: ["其他原因", "扫描器未能归类的原因"],
};

function skippedReason(reason: SkippedPathEntry["reason"], tr: (value: string) => string) {
  const [label, hint] = REASONS[reason] ?? REASONS.other;
  return { label: tr(label), hint: tr(hint) };
}

export function SkippedPaths({ summary }: { summary: AnalysisSummary }) {
  const { tr } = useI18n();
  const toast = useToast();
  const [showAll, setShowAll] = useState(false);
  const [explaining, setExplaining] = useState<SkippedPathEntry | null>(null);
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
                <span title={skippedReason(entry.reason, tr).hint}>{skippedReason(entry.reason, tr).label}<em> · {skippedReason(entry.reason, tr).hint}</em></span>
              </div>
              {entry.occurrences > 1 && (
                <span className="bd" title={tr("同一路径被多次跳过")}>×{entry.occurrences.toLocaleString()}</span>
              )}
              <button type="button" className="space-icon-button" title={tr("AI 解读：为什么跳过、要不要处理")}
                aria-label={`${tr("AI 解读")}: ${entry.path}`} onClick={() => setExplaining(entry)}>
                <i className="ti ti-sparkles" />
              </button>
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

      {explaining && <AiAskModal title={tr("为什么跳过这个路径")} sub={explaining.path}
        note={tr("只把路径和跳过原因发给 AI，不读取文件内容。")}
        saveAs={`space-skip:${explaining.reason}:${explaining.path}`}
        run={() => askAi("skipped_path", { path: explaining.path, reason: skippedReason(explaining.reason, (t) => t).label, count: String(explaining.occurrences) })}
        onClose={() => setExplaining(null)} />}

      {unlisted > 0 && (
        <div className="space-skipped-note">
          <i className="ti ti-info-circle" />
          {tr("为控制内存占用，另有")} {unlisted.toLocaleString()} {tr("条跳过记录未展开。")}
        </div>
      )}
    </section>
  );
}
