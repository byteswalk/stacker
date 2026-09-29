import { Modal } from "../../../ui";
import { useI18n } from "../../../i18n";
import { dismissCleanupResult, useCleanupStore } from "../cleanupStore";
import { formatSpaceBytes } from "./DevelopmentArtifacts";

/** 后端用翻译键报告清理结果，这里把键说成人话；未知的键原样显示，好让问题露出来。 */
const REASON_LABEL: Record<string, string> = {
  "spaceAnalysis.cleanup.reason.missing": "路径已不存在",
  "spaceAnalysis.cleanup.reason.outsideRoot": "路径不在已批准的扫描范围内",
  "spaceAnalysis.cleanup.reason.linkDetected": "发现链接或重解析点，已跳过",
  "spaceAnalysis.cleanup.reason.identityChanged": "扫描后路径已被替换",
  "spaceAnalysis.cleanup.reason.classificationChanged": "扫描后该项已不再是可清理类型",
  "spaceAnalysis.cleanup.reason.accessDenied": "拒绝访问",
  "spaceAnalysis.cleanup.reason.deleteFailed": "无法删除该项",
  "spaceAnalysis.cleanup.reason.cancelled": "清理已取消",
  "spaceAnalysis.cleanup.reason.elevationFailed": "管理员权限未获批准，或提权助手未完成",
};

const STATE_LABEL: Record<string, string> = {
  queued: "排队中",
  running: "进行中",
  cancelling: "正在取消",
  completed: "已完成",
  cancelled: "已取消",
  failed: "失败",
  pending: "待处理",
  skipped: "已跳过",
};

export const cleanupLabel = (key: string) => REASON_LABEL[key] ?? STATE_LABEL[key] ?? key;

export function CleanupResultModal({ onRescan }: { onRescan: (paths: string[]) => void }) {
  const { tr } = useI18n();
  const cleanup = useCleanupStore();
  const result = cleanup.result;
  if (!result) return null;
  const affected = result.items.filter((item) => item.state === "completed").map((item) => item.path);
  return <Modal wide title={tr("清理结果")} icon="ti-circle-check" onClose={dismissCleanupResult}
    footer={<>
      <button className="gh sm" onClick={dismissCleanupResult}>{tr("关闭")}</button>
      <button className="pr sm" disabled={affected.length === 0} onClick={() => { onRescan(affected); dismissCleanupResult(); }}><i className="ti ti-refresh" /> {tr("复查受影响目录")}</button>
    </>}>
    <div className="space-cleanup-result-summary"><b>{tr("实际释放")} {formatSpaceBytes(result.actualReleasedBytes)}</b><span>{tr("状态")}: {tr(cleanupLabel(result.state))}</span></div>
    <div className="space-cleanup-result-list">
      {result.items.map((item) => <div key={item.nodeId}><i className={`ti ${item.state === "completed" ? "ti-circle-check" : item.state === "failed" ? "ti-alert-circle" : "ti-info-circle"}`} />
        <span title={item.path}>{item.path}</span><b>{formatSpaceBytes(item.actualReleasedBytes)}</b><small>{tr(cleanupLabel(item.reasonKey ?? item.state))}</small></div>)}
    </div>
  </Modal>;
}
