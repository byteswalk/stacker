import { useEffect } from "react";
import { useToast } from "../../ui";
import { reportFrontendWarning } from "../../invoke";
import { refreshTools, vibeSnapshot } from "../agents/catalogStore";
import { initAgentTasks, type AgentTask } from "./taskStore";

export const ACTION_TEXT: Record<AgentTask["action"], string> = { install: "安装", update: "更新", uninstall: "卸载", repair: "修复" };

/** Shows one toast per finished agent task on any page and refreshes the affected cards. */
export function useTaskToasts() {
  const toast = useToast();
  useEffect(() => {
    let active = true;
    let dispose: (() => void) | undefined;
    void initAgentTasks((task) => {
      const action = ACTION_TEXT[task.action];
      if (task.state === "succeeded") toast(`${task.surfaceLabel} ${action}完成`, "ok");
      else if (task.state === "failed") toast(`${task.surfaceLabel} ${action}失败：${task.message ?? ""}（可在任务面板查看日志）`, "err");
      else toast(`已取消${task.surfaceLabel}${action}`, "info");
      // A shared CLI shows on several cards; refresh all of them.
      const siblings = task.cliId
        ? vibeSnapshot().tools.filter((tool) => tool.cli_id === task.cliId).map((tool) => tool.id)
        : [];
      void refreshTools([task.productId, ...siblings]).catch((cause) => reportFrontendWarning("Agent refresh after task failed", cause));
    }).then((unlisten) => {
      if (active) dispose = unlisten;
      else unlisten();
    }).catch((cause) => reportFrontendWarning("Agent task events are unavailable", cause));
    return () => { active = false; dispose?.(); };
  }, [toast]);
}
