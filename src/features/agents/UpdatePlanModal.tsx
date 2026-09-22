import { useEffect, useState } from "react";
import { invoke } from "../../invoke";
import { Loading, Modal, useBusyRead, useToast } from "../../ui";
import { refreshOneTool, vibeSnapshot } from "./catalogStore";
import type { AgentTask } from "../agent-tasks/taskStore";

type PlanItem = {
  productId: string;
  productName: string;
  surface: "cli" | "desktop";
  surfaceLabel: string;
  current?: string | null;
  latest?: string | null;
  reason?: string | null;
  lookupFailed?: boolean;
};
type UpdatePlan = { auto: PlanItem[]; manual: PlanItem[]; unknown?: PlanItem[] };

function PlanRow({ item, icons, onRetry }: {
  item: PlanItem;
  icons: Map<string, string>;
  onRetry?: (item: PlanItem) => Promise<void>;
}) {
  const icon = icons.get(item.productId);
  const [retrying, setRetrying] = useState(false);
  return (
    <li className={"plan-row" + (item.reason ? " manual" : "")}>
      <span className="plan-icon" aria-hidden="true">
        {icon ? <img src={`/brands/${icon}`} alt="" /> : <i className="ti ti-sparkles" />}
      </span>
      <span className="plan-name">
        <b>{item.surfaceLabel}</b>
        <small>{item.reason ?? `${item.productName} · ${item.surface === "cli" ? "CLI" : "桌面端"}`}</small>
      </span>
      <span className="plan-versions">
        <span className="plan-ver">{item.current ?? "未知"}</span>
        {item.latest && <>
          <i className="ti ti-arrow-narrow-right" aria-hidden="true" />
          <span className="plan-ver next">{item.latest}</span>
        </>}
      </span>
      {onRetry && item.lookupFailed && (
        <button className="gh sm plan-retry" disabled={retrying}
          onClick={() => { setRetrying(true); void onRetry(item).finally(() => setRetrying(false)); }}>
          <i className={"ti " + (retrying ? "ti-loader-2 spin" : "ti-refresh")} />{retrying ? "查询中" : "重试"}
        </button>
      )}
    </li>
  );
}

/** Lists what 一键更新 will run in the background and what needs the user. */
export function UpdatePlanModal({ onClose }: { onClose: () => void }) {
  const toast = useToast();
  const read = useBusyRead();
  const [plan, setPlan] = useState<UpdatePlan | null>(null);
  const [starting, setStarting] = useState(false);
  const icons = new Map(vibeSnapshot().tools.map((tool) => [tool.id, tool.icon ?? ""]));

  // Ask again for one install whose latest version could not be looked up, then re-plan:
  // an update found now moves it into the groups above.
  async function retry(item: PlanItem) {
    try {
      await refreshOneTool(item.productId);
      setPlan(await invoke<UpdatePlan>("agent_update_plan"));
    } catch (error) {
      toast(`重新查询失败：${error}`, "err");
    }
  }

  useEffect(() => {
    let active = true;
    read("正在读取更新计划", () => invoke<UpdatePlan>("agent_update_plan"))
      .then((next) => { if (active) setPlan(next); })
      .catch((error) => {
        toast(`读取更新计划失败：${error}`, "err");
        onClose();
      });
    return () => { active = false; };
  }, [onClose, read, toast]);

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

  return (
    <Modal title="一键更新" icon="ti-cloud-upload" onClose={starting ? undefined : onClose}
      sub={plan ? `${plan.auto.length} 项可在后台更新 · 同一安装器的任务自动排队，完成后逐项提示` : undefined}
      footer={<>
        <button className="gh" disabled={starting} onClick={onClose}>取消</button>
        <button className="pr" disabled={!plan || plan.auto.length === 0 || starting} onClick={() => void start()}>
          <i className={"ti " + (starting ? "ti-loader spin" : "ti-player-play")} />
          {starting ? "正在创建任务…" : `开始更新 ${plan?.auto.length ?? 0} 项`}
        </button>
      </>}>
      {!plan ? <Loading text="正在读取更新计划…" /> : (
        <div className="update-plan">
          {plan.auto.length > 0
            ? <ul>{plan.auto.map((item) => <PlanRow key={`${item.productId}-${item.surface}`} item={item} icons={icons} />)}</ul>
            : <div className="plan-empty"><i className="ti ti-circle-check" />
              {(plan.unknown?.length ?? 0) > 0 ? "能查到最新版本的智能体都已是最新。" : "所有可自动更新的智能体都已是最新版本。"}
            </div>}
          {plan.manual.length > 0 && <>
            <div className="plan-group"><i className="ti ti-hand-finger" /> 需要手动处理 <span>{plan.manual.length}</span></div>
            <ul>{plan.manual.map((item) => <PlanRow key={`${item.productId}-${item.surface}`} item={item} icons={icons} />)}</ul>
          </>}
          {(plan.unknown?.length ?? 0) > 0 && <>
            <div className="plan-group"><i className="ti ti-help-circle" /> 查不到最新版本 <span>{plan.unknown!.length}</span></div>
            <ul>{plan.unknown!.map((item) => <PlanRow key={`${item.productId}-${item.surface}`} item={item} icons={icons} onRetry={retry} />)}</ul>
          </>}
        </div>
      )}
    </Modal>
  );
}
