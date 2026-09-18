import { useEffect, useState } from "react";
import { invoke } from "../../invoke";
import { Loading, Modal, useToast } from "../../ui";
import type { AgentTask } from "../agent-tasks/taskStore";

type PlanItem = {
  productId: string;
  productName: string;
  surface: "cli" | "desktop";
  surfaceLabel: string;
  current?: string | null;
  latest?: string | null;
  reason?: string | null;
};
type UpdatePlan = { auto: PlanItem[]; manual: PlanItem[] };

/** Lists what 一键更新 will run in the background and what needs the user. */
export function UpdatePlanModal({ onClose }: { onClose: () => void }) {
  const toast = useToast();
  const [plan, setPlan] = useState<UpdatePlan | null>(null);
  const [starting, setStarting] = useState(false);

  useEffect(() => {
    let active = true;
    invoke<UpdatePlan>("agent_update_plan")
      .then((next) => { if (active) setPlan(next); })
      .catch((error) => {
        toast(`读取更新计划失败：${error}`, "err");
        onClose();
      });
    return () => { active = false; };
  }, [onClose, toast]);

  async function start() {
    setStarting(true);
    try {
      const tasks = await invoke<AgentTask[]>("agent_update_all");
      toast(`已创建 ${tasks.length} 个更新任务，可在任务面板查看进度`, "ok");
      onClose();
    } catch (error) {
      toast(`创建更新任务失败：${error}`, "err");
      setStarting(false);
    }
  }

  const row = (item: PlanItem) => (
    <li key={`${item.productId}-${item.surface}`}>
      <b>{item.surfaceLabel}</b>
      <span className="mono dim">{item.current ?? "?"} → {item.latest ?? "?"}</span>
      {item.reason && <span className="dim">{item.reason}</span>}
    </li>
  );

  return (
    <Modal title="一键更新" icon="ti-cloud-upload" wide onClose={starting ? undefined : onClose}
      footer={<>
        <button className="gh" disabled={starting} onClick={onClose}>取消</button>
        <button className="pr" disabled={!plan || plan.auto.length === 0 || starting} onClick={() => void start()}>
          {starting ? "正在创建任务…" : `开始更新 ${plan?.auto.length ?? 0} 项`}
        </button>
      </>}>
      {!plan ? <Loading text="正在读取更新计划…" /> : (
        <div className="update-plan">
          <div className="seclabel">将在后台更新</div>
          {plan.auto.length ? <ul>{plan.auto.map(row)}</ul> : <p className="dim">没有可自动更新的项目。</p>}
          {plan.manual.length > 0 && <>
            <div className="seclabel">需要手动处理</div>
            <ul>{plan.manual.map(row)}</ul>
          </>}
        </div>
      )}
    </Modal>
  );
}
