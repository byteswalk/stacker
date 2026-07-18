import { Modal } from "../../../ui";
import { useI18n } from "../../../i18n";
import { dismissCleanupResult, useCleanupStore } from "../cleanupStore";
import { formatSpaceBytes } from "./DevelopmentArtifacts";

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
    <div className="space-cleanup-result-summary"><b>{tr("实际释放")} {formatSpaceBytes(result.actualReleasedBytes)}</b><span>{tr("状态")}: {tr(result.state)}</span></div>
    <div className="space-cleanup-result-list">
      {result.items.map((item) => <div key={item.nodeId}><i className={`ti ${item.state === "completed" ? "ti-circle-check" : item.state === "failed" ? "ti-alert-circle" : "ti-info-circle"}`} />
        <span title={item.path}>{item.path}</span><b>{formatSpaceBytes(item.actualReleasedBytes)}</b><small>{tr(item.reasonKey ?? item.state)}</small></div>)}
    </div>
  </Modal>;
}
