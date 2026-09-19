import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ConfirmModal, useBusyRead } from "../../ui";
import { migrationDeleteBackup, migrationStatus } from "./api";
import { MigrationDialog } from "./MigrationDialog";
import { AGENT_LABEL } from "./sessionsView";
import { errorMessage, type LocationStatus } from "./types";

/** Where an agent's data lives now, in words. */
export function locationText(loc: LocationStatus, t: (s: string) => string): string {
  switch (loc.kind) {
    case "migrated": return `${t("已迁移")}：${loc.source} → ${loc.actual}`;
    case "incomplete": return `${t("上次迁移未完成")}：${loc.source}`;
    case "env": return `${t("由环境变量指定位置")}：${loc.actual}`;
    case "external_link": return `${t("已是其他工具创建的链接")}：${loc.source} → ${loc.actual}`;
    case "missing": return `${t("未找到数据目录")}：${loc.source}`;
    default: return `${t("位于")} ${loc.actual}`;
  }
}

/** Migrate / move back / drop the backup for one agent, with their dialogs. */
export function LocationActions({ loc, onChanged }: { loc: LocationStatus; onChanged: () => void }) {
  const { tr: t } = useI18n();
  const [migrating, setMigrating] = useState<"migrate" | "back" | null>(null);
  const [dropBackup, setDropBackup] = useState(false);
  const [dropping, setDropping] = useState(false);
  const [error, setError] = useState("");

  return <>
    {loc.kind === "normal" && <button className="gh sm" onClick={() => setMigrating("migrate")}><i className="ti ti-transfer" />{t("迁移到其他盘…")}</button>}
    {loc.kind === "migrated" && loc.backupExists && <button className="pr sm" onClick={() => setDropBackup(true)}><i className="ti ti-trash" />{t("删除原位置的备份")}</button>}
    {loc.kind === "migrated" && <button className="gh sm" onClick={() => setMigrating("back")}>{t("迁回原位置")}</button>}
    {loc.kind === "incomplete" && <button className="pr sm" onClick={() => setMigrating("back")}><i className="ti ti-alert-triangle" />{t("上次迁移未完成，恢复原状")}</button>}
    {error && <small role="alert" className="warn">{t(error)}</small>}
    {migrating && <MigrationDialog status={loc} mode={migrating} onClose={(changed) => { setMigrating(null); if (changed) onChanged(); }} />}
    {dropBackup && <ConfirmModal title={t("删除原位置的备份")} icon="ti-trash" danger busy={dropping}
      message={`${t("确认智能体在新位置运行正常后再删除。删除后迁回需要重新复制数据。")} ${loc.backup}`}
      confirmLabel={t("删除备份")}
      onConfirm={() => {
        setDropping(true); setError("");
        migrationDeleteBackup(loc.agent)
          .then(() => { setDropBackup(false); onChanged(); })
          .catch((e) => setError(errorMessage(e)))
          .finally(() => setDropping(false));
      }}
      onClose={() => setDropBackup(false)} />}
  </>;
}

/** Settings block: move each agent's whole data folder to another drive. */
export function DataLocations({ onChanged }: { onChanged: () => void }) {
  const { tr: t } = useI18n();
  const read = useBusyRead();
  const [locations, setLocations] = useState<LocationStatus[] | null>(null);
  const [error, setError] = useState("");

  const load = useCallback(() => {
    read("正在读取数据位置", migrationStatus)
      .then((list) => { setLocations(list ?? []); setError(""); })
      .catch((e) => setError(errorMessage(e)));
  }, [read]);
  useEffect(() => { load(); }, [load]);

  return <div className="session-source data-locations">
    <div className="session-source-head">
      <b>{t("数据目录迁移")}</b>
      <small>{t("C 盘空间不够时，把智能体的全部数据（历史会话、配置、缓存）移动到其他盘。原位置会留下一个目录联接指向新位置，智能体照常使用，历史记录不会丢失。迁移前需要先退出对应的智能体。")}</small>
    </div>
    {error && <p role="alert" className="session-error">{t(error)}</p>}
    {locations?.map((loc) => <div className="data-location" key={loc.agent}>
      <div className="data-location-text">
        <b>{AGENT_LABEL[loc.agent]}</b>
        <code title={locationText(loc, t)}>{locationText(loc, t)}</code>
        {loc.kind === "migrated" && loc.backupExists && <small className="warn">{t("C 盘空间还没有释放：原位置保留了一份备份。确认智能体在新位置运行正常后，删除备份即可释放。")}</small>}
      </div>
      <div className="session-actions"><LocationActions loc={loc} onChanged={() => { load(); onChanged(); }} /></div>
    </div>)}
  </div>;
}
