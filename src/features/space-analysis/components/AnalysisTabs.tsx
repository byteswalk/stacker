import { useEffect, useState } from "react";
import { useI18n } from "../../../i18n";
import { invoke } from "../../../invoke";
import { useBusyRead } from "../../../ui";
import type { AnalysisSummary, ScanRequest, SnapshotMetadata, VolumeInfo } from "../types";
import { loadCleanupCandidates, useCleanupStore } from "../cleanupStore";
import { startScan } from "../store";
import { CacheDownloads } from "./CacheDownloads";
import { CleanupPlanModal } from "./CleanupPlanModal";
import { CleanupResultModal } from "./CleanupResultModal";
import { DevelopmentArtifacts } from "./DevelopmentArtifacts";
import { DevelopmentProjects } from "./DevelopmentProjects";
import { DuplicateFiles } from "./DuplicateFiles";
import { LargeFiles } from "./LargeFiles";
import { SkippedPaths } from "./SkippedPaths";
import { SpaceOverview } from "./SpaceOverview";
import { SpaceChanges } from "./SpaceChanges";
import { SpaceMonitorModal } from "./SpaceMonitorModal";

// What can be cleaned comes right after the projects it belongs to; the directory ranking is a view of the overview.
export const ANALYSIS_TABS = ["overview", "projects", "development-artifacts", "cache-downloads", "large-files", "duplicates", "skipped-paths", "changes"] as const;
type AnalysisTab = (typeof ANALYSIS_TABS)[number];

type SpaceAnalysisSettings = { large_file_threshold_bytes: number };
const DEFAULT_LARGE_FILE_THRESHOLD = 1024 ** 3;
const savedSnapshots = new Map<string, SnapshotMetadata | null>();

function normalizedRoot(value: string) {
  return value.trim().replaceAll("/", "\\").replace(/\\+$/, "").toLocaleLowerCase("en-US");
}

export function matchedFreeBytes(request: ScanRequest, volumes: readonly VolumeInfo[]): number | null {
  if (request.mode !== "drives") return null;
  const selected = new Set(request.targets.map(normalizedRoot));
  if (selected.size === 0) return null;
  const matched = volumes.filter((volume) => volume.fixed && selected.has(normalizedRoot(volume.root)));
  return matched.length === selected.size ? matched.reduce((sum, volume) => sum + volume.freeBytes, 0) : null;
}

export function AnalysisTabs({ taskId, request }: { taskId: string; request: ScanRequest }) {
  const { t, tr } = useI18n();
  const read = useBusyRead();
  const cleanup = useCleanupStore();
  const [activeTab, setActiveTab] = useState<AnalysisTab>("overview");
  const [summary, setSummary] = useState<AnalysisSummary | null>(null);
  const [freeBytes, setFreeBytes] = useState<number | null>(null);
  const [largeFileThreshold, setLargeFileThreshold] = useState(DEFAULT_LARGE_FILE_THRESHOLD);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useState<SnapshotMetadata | null>(savedSnapshots.get(taskId) ?? null);
  const [monitorOpen, setMonitorOpen] = useState(false);

  useEffect(() => {
    let current = true;
    setActiveTab("overview");
    setSummary(null);
    setFreeBytes(null);
    setLoading(true);
    setError(null);
    const requests: [Promise<AnalysisSummary>, Promise<VolumeInfo[]>, Promise<SpaceAnalysisSettings>] = [
      invoke<AnalysisSummary>("space_scan_summary", { taskId }),
      request.mode === "drives" ? invoke<VolumeInfo[]>("space_fixed_volumes") : Promise.resolve([]),
      invoke<SpaceAnalysisSettings>("settings_get"),
    ];
    void read("正在读取分析结果", () => Promise.allSettled(requests)).then(([summaryResult, volumeResult, settingsResult]) => {
      if (!current) return;
      if (summaryResult.status === "rejected") {
        setError(tr("无法读取本次空间分析结果，请重新扫描。"));
        setLoading(false);
        return;
      }
      setSummary(summaryResult.value);
      if (!savedSnapshots.has(taskId)) {
        void invoke<SnapshotMetadata | null>("space_snapshot_save", { taskId }).then((metadata) => {
          savedSnapshots.set(taskId, metadata);
          if (current) setSnapshot(metadata);
        }).catch(() => undefined);
      }
      if (volumeResult.status === "fulfilled") setFreeBytes(matchedFreeBytes(request, volumeResult.value));
      if (settingsResult.status === "fulfilled") setLargeFileThreshold(Math.max(1, settingsResult.value.large_file_threshold_bytes));
      setLoading(false);
    });
    void read("正在读取可清理项", () => loadCleanupCandidates(taskId));
    return () => { current = false; };
  }, [read, request, taskId, tr]);

  if (loading) return <div className="space-analysis-state" aria-live="polite"><i className="ti ti-loader spin" /><span>{tr("正在读取分析结果…")}</span></div>;
  if (error || !summary) return <div className="space-analysis-state error" role="alert"><i className="ti ti-alert-triangle" /><span>{error ?? tr("本次空间分析结果不可用，请重新扫描。")}</span></div>;

  const labels: Record<AnalysisTab, string> = {
    projects: t("space.projects.tab"),
    overview: tr("空间概览"), "large-files": tr("大文件"),
    duplicates: tr("重复文件"),
    "development-artifacts": tr("开发产物"), "cache-downloads": tr("缓存与下载"),
    "skipped-paths": tr("已跳过路径"),
    changes: tr("空间变化"),
  };
  const cacheImpactKeys = new Set(["spaceAnalysis.impact.nodeDependencies", "spaceAnalysis.impact.gradleProjectCache"]);
  const cacheNodes = cleanup.candidates.filter((node) => cacheImpactKeys.has(node.impactKey ?? ""));
  const artifactNodes = cleanup.candidates.filter((node) => !cacheImpactKeys.has(node.impactKey ?? ""));

  function rescanAffected(paths: string[]) {
    const targets = [...new Set(paths.map((path) => path.replace(/[\\/][^\\/]+[\\/]?$/, "")).filter(Boolean))];
    if (targets.length > 0) void startScan({ mode: "directories", targets });
  }

  return <section className="space-analysis-results">
    <div className="space-analysis-tab-head">
      <div className="space-analysis-tabs" role="tablist" aria-label={tr("空间分析视图")}>
        {ANALYSIS_TABS.map((tab) => <button key={tab} type="button" role="tab" aria-selected={activeTab === tab}
          className={activeTab === tab ? "active" : ""} onClick={() => setActiveTab(tab)}>{labels[tab]}</button>)}
      </div>
      <div className="space-cleanup-toolbar">
        <button className="gh sm" type="button" onClick={() => setMonitorOpen(true)} title={tr("监控选定目录的空间变化")}><i className="ti ti-activity" /> {tr("磁盘实时监控")}</button>
      </div>
    </div>
    <div className="space-analysis-tab-panel" role="tabpanel">
      {activeTab === "overview" && <SpaceOverview taskId={taskId} summary={summary} freeBytes={freeBytes} />}
      {activeTab === "projects" && <DevelopmentProjects projects={summary.projects} />}
      {activeTab === "large-files" && <LargeFiles taskId={taskId} thresholdBytes={largeFileThreshold} />}
      {activeTab === "duplicates" && <DuplicateFiles taskId={taskId} />}
      {activeTab === "development-artifacts" && <DevelopmentArtifacts nodes={artifactNodes} />}
      {activeTab === "cache-downloads" && <CacheDownloads nodes={cacheNodes} />}
      {activeTab === "skipped-paths" && <SkippedPaths summary={summary} />}
      {activeTab === "changes" && (snapshot
        ? <SpaceChanges fingerprint={snapshot.targetFingerprint} />
        : <div className="space-analysis-empty">{tr("空间快照已关闭，或本次扫描尚未生成快照。")}</div>)}
    </div>
    <CleanupPlanModal />
    <CleanupResultModal onRescan={rescanAffected} />
    {monitorOpen && <SpaceMonitorModal roots={summary.targets} onClose={() => setMonitorOpen(false)} />}
  </section>;
}
