import { useState } from "react";
import { Modal, useToast } from "../../../ui";
import { useI18n } from "../../../i18n";
import { dismissCleanupPlan, startCleanup, useCleanupStore } from "../cleanupStore";
import { candidateImpact, formatSpaceBytes } from "./DevelopmentArtifacts";

export function CleanupPlanModal() {
  const { tr } = useI18n();
  const toast = useToast();
  const cleanup = useCleanupStore();
  const [confirmed, setConfirmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const plan = cleanup.plan;
  if (!plan || cleanup.result) return null;
  const destructive = plan.items.some((item) => item.safety !== "safe");
  return <Modal wide title={tr("确认清理计划")} icon="ti-eraser" onClose={busy ? undefined : dismissCleanupPlan}
    footer={<>
      <button className="gh sm" disabled={busy} onClick={dismissCleanupPlan}>{tr("取消")}</button>
      <button className={`pr sm${destructive ? " danger-solid" : ""}`} disabled={busy || !confirmed}
        onClick={async () => {
          setBusy(true);
          try {
            await startCleanup();
          } catch (error) {
            toast(`${tr("清理失败：")}${String(error)}`, "err");
          } finally {
            setBusy(false);
          }
        }}>
        <i className={busy ? "ti ti-loader spin" : "ti ti-eraser"} /> {busy ? tr("清理中…") : tr("开始清理")}
      </button>
    </>}>
    <div className={`space-cleanup-plan${busy ? " trace-card" : ""}`}>
      {busy && <span className="border-runner" aria-hidden="true" />}
      <div className="space-cleanup-plan-summary">
        <b>{plan.items.length} {tr("项")}</b><span>{tr("预计释放")} {formatSpaceBytes(plan.estimatedBytes)}</span>
        {plan.elevationRequirement === "required" && <span className="bd w"><i className="ti ti-shield-lock" /> {tr("部分项目需要管理员权限")}</span>}
      </div>
      <div className="space-cleanup-plan-groups">
        {plan.items.map((item) => <div key={item.nodeId} className="space-cleanup-plan-item">
          <span className={`space-safety-dot ${item.safety}`} />
          <div><strong title={item.path}>{item.path}</strong><small>{tr(candidateImpact({ impactKey: item.impactKey }))}</small></div>
          <b>{formatSpaceBytes(item.estimatedBytes)}</b>
        </div>)}
      </div>
      {cleanup.progress && <div className="space-cleanup-progress">
        <span>{tr("已处理")} {cleanup.progress.completedItems} / {cleanup.progress.totalItems}</span>
        <span>{tr("已释放")} {formatSpaceBytes(cleanup.progress.actualReleasedBytes)}</span>
      </div>}
      <label className="space-cleanup-confirm"><input type="checkbox" checked={confirmed} disabled={busy} onChange={(event) => setConfirmed(event.target.checked)} />
        <span>{tr("我已确认所选目录和影响，允许执行此清理计划。")}</span>
      </label>
    </div>
  </Modal>;
}
