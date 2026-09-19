import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { Modal } from "../../ui";
import { formatSpaceBytes as bytes } from "../space-analysis/components/SpaceOverview";
import { migrationCancel, migrationCheck, migrationJob, migrationMoveBack, migrationStart } from "./api";
import { AGENT_LABEL } from "./sessionsView";
import { errorMessage, type LocationStatus, type MigrationCheck, type MigrationJob } from "./types";

const APP_HINT: Record<string, string> = {
  codex: "请先完全退出 Codex 命令行和 Codex 桌面端。",
  claude: "请先完全退出 Claude Code 命令行和 Claude 桌面端（桌面端的 Code 页也使用这个目录）。",
};

/** Migrate to another drive, move back, or undo an interrupted migration. */
export function MigrationDialog({ status, mode, onClose }: { status: LocationStatus; mode: "migrate" | "back"; onClose: (changed: boolean) => void }) {
  const { tr: t } = useI18n();
  const [target, setTarget] = useState(status.suggestedTarget);
  const [check, setCheck] = useState<MigrationCheck | null>(null);
  const [checking, setChecking] = useState(false);
  const [job, setJob] = useState<MigrationJob | null>(null);
  const [error, setError] = useState("");
  const name = AGENT_LABEL[status.agent];

  async function runCheck(path = target) {
    setChecking(true); setError("");
    try { setCheck(await migrationCheck(status.agent, path)); } catch (e) { setError(errorMessage(e)); } finally { setChecking(false); }
  }
  useEffect(() => {
    if (mode !== "migrate") return;
    migrationCheck(status.agent, status.suggestedTarget).then(setCheck).catch((e) => setError(errorMessage(e)));
  }, [mode, status.agent, status.suggestedTarget]);

  const running = job?.state === "running";
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => {
      void migrationJob().then((next) => { if (next) setJob(next); }).catch((e) => setError(errorMessage(e)));
    }, 500);
    return () => clearInterval(timer);
  }, [running]);

  async function choose() {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked === "string") { const next = `${picked.replace(/[\\/]$/, "")}\\${status.agent}`; setTarget(next); void runCheck(next); }
  }
  async function start() {
    setError("");
    try { setJob(mode === "migrate" ? await migrationStart(status.agent, target) : await migrationMoveBack(status.agent)); }
    catch (e) { setError(errorMessage(e)); }
  }

  const incomplete = status.kind === "incomplete";
  const withName = (text: string) => t(text).replace("{agent}", name);
  const title = withName(mode === "migrate" ? "迁移 {agent} 数据到其他盘" : incomplete ? "恢复 {agent} 数据原状" : "把 {agent} 数据迁回原位置");
  const ready = mode === "back" || (!!check && !check.problems.length && !checking);
  const done = job && !running;

  return <Modal wide title={title} icon="ti-transfer" onClose={running ? undefined : () => onClose(!!job)} footer={job
    ? <>{running && <button className="gh sm" onClick={() => void migrationCancel()}>{t("取消")}</button>}<button className="pr sm" disabled={running} onClick={() => onClose(true)}>{t("完成")}</button></>
    : <><button className="gh sm" onClick={() => onClose(false)}>{t("取消")}</button><button className="pr sm" disabled={!ready} onClick={() => void start()}><i className="ti ti-transfer" />{t(mode === "migrate" ? "开始迁移" : incomplete ? "恢复原状" : "开始迁回")}</button></>}>
    <div className="session-delete">
      {!job ? <>
        {mode === "migrate" ? <>
          <p className="session-note">{t("数据会复制到新位置并核对，原位置改为指向新位置的目录联接。命令行、桌面端和其他工具照常使用原路径，无需任何设置。原目录先保留为备份，确认智能体正常后再删除以释放空间。")}</p>
          <div className="migration-row"><span>{t("当前位置")}</span><code>{status.source}</code></div>
          <label className="migration-row"><span>{t("新位置")}</span>
            <input className="ip" value={target} onChange={(e) => { setTarget(e.target.value); setCheck(null); }} onBlur={() => void runCheck()} />
            <button className="gh sm" onClick={() => void choose().catch((e) => setError(errorMessage(e)))}><i className="ti ti-folder" />{t("选择…")}</button>
          </label>
          {!!status.drives.length && <div className="migration-drives">{status.drives.map((d) => <button key={d.root} className="session-tag link" onClick={() => { const next = `${d.root}AgentData\\${status.agent}`; setTarget(next); void runCheck(next); }}>{d.root} {t("剩余")} {bytes(d.free)}</button>)}</div>}
          {checking && <p className="session-note"><i className="ti ti-loader spin" /> {t("正在检查…")}</p>}
          {check && !checking && <>
            <p className="session-impact">{t("将迁移")} <b>{bytes(check.bytes)}</b> · {check.files} {t("个文件")}{check.free > 0 && <> · {t("目标剩余")} {bytes(check.free)}</>}</p>
            {check.problems.map((p) => <p key={p} role="alert" className="session-error"><i className="ti ti-alert-triangle" />{t(errorMessage(p))}{p === "E_APP_RUNNING" && <> {t(APP_HINT[status.agent])}</>}{p === "E_APP_RUNNING" && <button className="gh sm" onClick={() => void runCheck()}>{t("重新检查")}</button>}</p>)}
            {!check.problems.length && <p className="session-note"><i className="ti ti-circle-check" /> {withName("检查通过。迁移期间不要启动 {agent}。")}</p>}
          </>}
        </> : <>
          {incomplete
            ? <p className="session-note">{t("上次迁移没有完成。恢复原状会删除目录联接、把备份改回原名，并删除新位置上的副本。")}</p>
            : status.backupExists
              ? <p className="session-note">{t("原位置的备份还在，迁回只需删除目录联接并把备份改回原名，瞬间完成。新位置上的副本会保留，可自行删除。")}</p>
              : <p className="session-note">{t("备份已删除，需要把数据从新位置复制回原位置，耗时取决于数据大小。新位置上的副本会保留，可自行删除。")}</p>}
          <div className="migration-row"><span>{t("原位置")}</span><code>{status.source}</code></div>
          <div className="migration-row"><span>{t("新位置")}</span><code>{status.target || status.actual}</code></div>
          <p className="session-note">{t(APP_HINT[status.agent])}</p>
        </>}
      </> : <>
        <p className="session-impact"><b>{t(running ? "进行中" : job.state === "completed" ? "已完成" : "失败")}</b>{job.total > 0 && <> · {bytes(job.copied)} / {bytes(job.total)}</>}</p>
        {running && job.total > 0 && <progress max={job.total} value={job.copied} />}
        {done && job.state === "completed" && mode === "migrate" && <p className="session-note"><i className="ti ti-circle-check" /> {withName("迁移完成。请打开 {agent} 确认会话和登录都正常，然后在「占用」标签中删除原位置的备份以释放空间。")}</p>}
        {done && job.state === "completed" && mode === "back" && <p className="session-note"><i className="ti ti-circle-check" /> {t("数据已回到原位置。")}</p>}
        {job.error && <p role="alert" className="session-error">{t(errorMessage(job.error))} {t("已恢复到迁移前的状态。")}</p>}
      </>}
      {error && <p role="alert" className="session-error">{t(error)}</p>}
    </div>
  </Modal>;
}
