import { useEffect, useState } from "react";
import { invoke, reportFrontendWarning } from "../../invoke";
import { Modal, useToast } from "../../ui";
import { useI18n } from "../../i18n";

type Move = { from: string; to: string; conflict: boolean };
type EnvChange = { system: boolean; name: string; before: string; after: string };
type Plan = { moves: Move[]; env: EnvChange[]; needsAdmin: boolean };
type Outcome = { moved: number; envChanged: number; failures: string[] };

const DISMISS_KEY = "stacker.toolRelocation.later";

/**
 * Tools an older portable build installed beside itself, offered for a move to the folder every
 * build now uses. Shown once per start while there is something to do; nothing moves on its own.
 */
export function ToolRelocation() {
  const { tr } = useI18n();
  const toast = useToast();
  const [plan, setPlan] = useState<Plan | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let later = false;
    try { later = sessionStorage.getItem(DISMISS_KEY) === "1"; } catch { /* per-viewer convenience only */ }
    if (later) return;
    invoke<Plan>("tool_relocation_plan")
      .then((next) => { if (next.moves.length || next.env.length) setPlan(next); })
      .catch((error) => reportFrontendWarning("Unable to check where tools are installed.", error));
  }, []);

  if (!plan) return null;

  function close() {
    try { sessionStorage.setItem(DISMISS_KEY, "1"); } catch { /* per-viewer convenience only */ }
    setPlan(null);
  }

  async function apply() {
    setBusy(true);
    try {
      const outcome = await invoke<Outcome>("tool_relocation_apply");
      if (outcome.failures.length) {
        toast(tr("迁移没有全部完成：") + outcome.failures.join("；"), "err");
      } else {
        toast(tr("已迁移 {moved} 项，更新 {env} 个环境变量；新开的终端即可生效")
          .replace("{moved}", String(outcome.moved)).replace("{env}", String(outcome.envChanged)), "ok");
        setPlan(null);
      }
    } catch (error) {
      toast(tr("迁移失败：") + String(error), "err");
    } finally {
      setBusy(false);
    }
  }

  const moves = plan.moves.filter((item) => !item.conflict);
  const kept = plan.moves.filter((item) => item.conflict);
  return <Modal title={tr("工具位置需要迁移")} icon="ti-truck-delivery" wide onClose={busy ? undefined : close}
    footer={<>
      <button className="gh sm" onClick={close} disabled={busy}>{tr("稍后")}</button>
      <button className="pr sm" onClick={() => void apply()} disabled={busy}>
        {busy ? tr("迁移中…") : plan.needsAdmin ? tr("迁移（需要一次管理员授权）") : tr("迁移")}
      </button>
    </>}>
    <div className="reloc">
      <p className="reloc-lead">{tr("以前的免安装版把装好的工具放在程序自己的文件夹里，换新版本或删掉旧版本文件夹后就会丢失。现在所有工具统一放在本机数据目录，下面这些需要搬过去：")}</p>
      {moves.length > 0 && <section>
        <h4>{tr("搬移文件夹")}</h4>
        {moves.map((item) => <div key={item.from} className="reloc-row">
          <code title={item.from}>{item.from}</code><i className="ti ti-arrow-right" /><code title={item.to}>{item.to}</code>
        </div>)}
      </section>}
      {kept.length > 0 && <section>
        <h4>{tr("目标已存在，保留原处")}</h4>
        {kept.map((item) => <div key={item.from} className="reloc-row"><code title={item.from}>{item.from}</code></div>)}
      </section>}
      {plan.env.length > 0 && <section>
        <h4>{tr("环境变量")}</h4>
        {plan.env.map((change) => <div key={(change.system ? "s:" : "u:") + change.name} className="reloc-env">
          <b>{change.name}</b><span className="reloc-scope">{tr(change.system ? "系统" : "用户")}</span>
          <span className="reloc-what">{change.after ? tr("改为指向新位置，失效的旧路径一并去掉") : tr("指向的文件夹已不存在，删除该变量")}</span>
        </div>)}
      </section>}
    </div>
  </Modal>;
}
