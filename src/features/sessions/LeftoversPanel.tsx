import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { useBusyRead, useToast } from "../../ui";
import { formatSpaceBytes as bytes } from "../space-analysis/components/SpaceOverview";
import { saveLeftoverSettings, scanLeftovers } from "./api";
import { LeftoversDialog } from "./LeftoversDialog";
import { AGENT_LABEL, formatAge } from "./sessionsView";
import { errorMessage, type LeftoverItem, type LeftoverKind, type LeftoverReport, type LeftoverSettings, type ProjectLeftovers } from "./types";

// Survives tab switches; a scan reads every project the sessions worked in.
let cachedReport: LeftoverReport | null = null;

const KIND_ICON: Record<LeftoverKind, string> = {
  build: "ti-tools",
  stale: "ti-clock-hour-4",
  incremental: "ti-bolt",
  environment: "ti-brand-python",
  temp: "ti-files",
  checkpoints: "ti-history",
  images: "ti-photo",
};

/** What a scan ticks: everything recommended that nothing is running in. */
export function defaultLeftovers(report: LeftoverReport): string[] {
  return report.projects.flatMap((p) => p.items).filter((i) => i.recommended && !i.blocked).map((i) => i.id);
}

/** The build folder that old profiles or the incremental cache are part of. */
export function parentOf(id: string): string | null {
  const match = /^(.*):(stale|incremental)$/.exec(id);
  return match ? match[1].replace(/\|(Stale|Incremental)\|/, "|Build|") : null;
}

/** A part counts only when its whole folder is not being cleaned anyway. */
export function covered(id: string, ids: Set<string>): boolean {
  const parent = parentOf(id);
  return parent !== null && ids.has(parent);
}

export function sizeOf(items: LeftoverItem[], ids: Set<string>): number {
  return items.filter((i) => ids.has(i.id) && !covered(i.id, ids)).reduce((n, i) => n + i.bytes, 0);
}

type Draft = Pick<LeftoverSettings, "inactiveDays" | "checkpointDays" | "slim" | "slimDays" | "auto">;
const draftOf = (s: LeftoverSettings): Draft => ({ inactiveDays: s.inactiveDays, checkpointDays: s.checkpointDays, slim: s.slim, slimDays: s.slimDays, auto: s.auto });

function Days({ value, onChange, label }: { value: number; onChange: (days: number) => void; label: string }) {
  return <input className="ip leftovers-days" type="number" min={1} max={365} value={value} aria-label={label}
    onChange={(e) => onChange(Math.min(365, Math.max(1, Number(e.target.value) || 1)))} />;
}

export function LeftoversPanel() {
  const { tr: t } = useI18n();
  const toast = useToast();
  const read = useBusyRead();
  const [report, setReport] = useState<LeftoverReport | null>(cachedReport);
  const [selected, setSelected] = useState<Set<string>>(() => new Set(cachedReport ? defaultLeftovers(cachedReport) : []));
  const [draft, setDraft] = useState<Draft | null>(cachedReport ? draftOf(cachedReport.settings) : null);
  const [open, setOpen] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [cleaning, setCleaning] = useState<string[] | null>(null);
  const request = useRef(0);
  const [now] = useState(() => Date.now() / 1000);

  const load = useCallback(async (refresh: boolean) => {
    const current = ++request.current;
    setLoading(true); setError("");
    try {
      const next = await read("正在查找会话残留", () => scanLeftovers(refresh), "逐个查看 Claude Code 和 Codex 会话用过的项目目录，项目多时要一两分钟。");
      if (current !== request.current) return;
      cachedReport = next; setReport(next);
      setSelected(new Set(defaultLeftovers(next)));
      setDraft(draftOf(next.settings));
    } catch (e) {
      if (current === request.current) setError(errorMessage(e));
    } finally {
      if (current === request.current) setLoading(false);
    }
  }, [read]);

  useEffect(() => { if (!cachedReport) void load(false); }, [load]);

  async function saveSettings() {
    if (!draft) return;
    setSaving(true);
    try {
      await saveLeftoverSettings(draft);
      toast(t("设置已保存，正在按新设置重新查找"), "ok");
      await load(true);
    } catch (e) { toast(t(errorMessage(e)), "err"); }
    finally { setSaving(false); }
  }

  const items = report?.projects.flatMap((p) => p.items) ?? [];
  const picked = items.filter((i) => selected.has(i.id) && !covered(i.id, selected));
  const settingsDirty = !!report && !!draft && JSON.stringify(draft) !== JSON.stringify(draftOf(report.settings));
  const toggle = (ids: string[], on: boolean) => setSelected((old) => {
    const next = new Set(old);
    ids.forEach((id) => on ? next.add(id) : next.delete(id));
    return next;
  });
  const lastAuto = report?.settings.autoLastRun
    ? report.settings.autoLastError
      ? `${t("上次自动清理失败")}：${t(errorMessage(report.settings.autoLastError))}`
      : `${t("上次自动清理")} ${t(formatAge(report.settings.autoLastRun, now))}，${t("释放")} ${bytes(report.settings.autoLastFreed)}`
    : "";

  const status = (p: ProjectLeftovers) => p.skipped
    ? <span className="leftovers-chip muted" title={t(p.skipped)}>{t("未查看目录")}</span>
    : p.active
      ? <span className="leftovers-chip active">{t("在用")}</span>
      : <span className="leftovers-chip idle">{t("闲置")}</span>;

  return <div className="leftovers">
    <div className="footprint-summary">
      <div>
        <span>{t("会话残留共")}</span><b>{report ? bytes(report.total) : "—"}</b>
        <span>{t("默认勾选可释放")}</span><b className="accent">{report ? bytes(report.recommended) : "—"}</b>
      </div>
      <div className="session-actions">
        <button className="gh sm" disabled={loading} onClick={() => void load(true)}><i className={`ti ${loading ? "ti-loader spin" : "ti-refresh"}`} />{t(loading ? "正在查找…" : "重新查找")}</button>
        <button className="pr sm danger" disabled={!picked.length || loading} onClick={() => setCleaning(picked.map((i) => i.id))}>
          <i className="ti ti-trash" />{t("清理所选")}（{picked.length} {t("项")}{picked.length ? ` · ${bytes(sizeOf(items, selected))}` : ""}）
        </button>
      </div>
    </div>
    <p className="session-note leftovers-intro">{t("只看 Claude Code 和 Codex。编译产物、依赖和临时目录重建不花 token，只花编译或下载的时间；会话正文不删，最多去掉里面的截图。")}</p>

    {draft && <div className="leftovers-settings">
      <label>{t("项目闲置")} <Days value={draft.inactiveDays} label={t("项目闲置天数")} onChange={(inactiveDays) => setDraft({ ...draft, inactiveDays })} /> {t("天后清编译产物和临时目录")}</label>
      <label>{t("检查点保留")} <Days value={draft.checkpointDays} label={t("检查点保留天数")} onChange={(checkpointDays) => setDraft({ ...draft, checkpointDays })} /> {t("天")}</label>
      <label title={t("把会话记录里的截图换成 1 像素小图，文字全部保留，照样能查看和续聊。")}>
        <input type="checkbox" checked={draft.slim} onChange={(e) => setDraft({ ...draft, slim: e.target.checked })} /> {t("去掉")}
        <Days value={draft.slimDays} label={t("截图瘦身天数")} onChange={(slimDays) => setDraft({ ...draft, slimDays })} /> {t("天前会话里的截图")}
      </label>
      <label title={t("每天最多一次，只清默认勾选的项；有程序在用的目录会跳过。")}>
        <input type="checkbox" checked={draft.auto} onChange={(e) => setDraft({ ...draft, auto: e.target.checked })} /> {t("每天自动清理")}
      </label>
      {lastAuto && <span className="s dim">{lastAuto}</span>}
      <button className="gh sm" disabled={!settingsDirty || saving} onClick={() => void saveSettings()}><i className="ti ti-device-floppy" />{t("保存设置")}</button>
    </div>}

    {error && <div role="alert" className="session-error">{t(error)}</div>}
    {!report && loading && <div className="session-empty"><i className="ti ti-loader spin" /><b>{t("正在查找会话残留，项目多时要一两分钟…")}</b></div>}
    {report && !report.projects.length && <div className="session-empty"><i className="ti ti-circle-check" /><b>{t("没有找到会话残留")}</b></div>}

    {report?.projects.map((project) => {
      const ids = project.items.filter((i) => !i.blocked).map((i) => i.id);
      const chosen = ids.filter((id) => selected.has(id));
      const total = project.items.reduce((n, i) => n + i.bytes, 0);
      const expanded = open === project.key;
      return <section key={project.key} className="leftovers-project">
        <header>
          <input type="checkbox" disabled={!ids.length} checked={!!ids.length && chosen.length === ids.length}
            ref={(box) => { if (box) box.indeterminate = chosen.length > 0 && chosen.length < ids.length; }}
            aria-label={t(project.name)} onChange={(e) => toggle(ids, e.target.checked)} />
          <button className="leftovers-name" onClick={() => setOpen(expanded ? null : project.key)}>
            <b>{t(project.name)}</b>
            <small title={project.path}>{project.path}</small>
          </button>
          <span className="leftovers-meta">
            {project.agents.map((agent) => <span key={agent} className="leftovers-chip">{AGENT_LABEL[agent] ?? agent}</span>)}
            {project.sessions > 0 && <span>{project.sessions} {t("个会话")}</span>}
            {project.lastActivity > 0 && <span>{t(formatAge(project.lastActivity, now))}</span>}
            {status(project)}
          </span>
          <span className="leftovers-size"><b>{bytes(sizeOf(project.items, selected))}</b><small>/ {bytes(total)}</small></span>
          <button className="gh xs" aria-label={t(expanded ? "收起" : "展开")} onClick={() => setOpen(expanded ? null : project.key)}>
            <i className={"ti " + (expanded ? "ti-chevron-up" : "ti-chevron-down")} />
          </button>
        </header>
        {expanded && <div className="leftovers-items">
          {project.skipped && <p className="session-note"><i className="ti ti-info-circle" /> {t("没有查看这个项目的目录")}：{t(project.skipped)}</p>}
          {project.items.map((item) => <div key={item.id} className={"leftovers-item" + (item.blocked ? " blocked" : "")}>
            {item.blocked
              ? <span className="fp-lock" title={t(item.blocked)}><i className="ti ti-lock" /></span>
              : covered(item.id, selected)
                ? <input type="checkbox" checked disabled aria-label={t(item.label)} title={t("所在的构建产物已整个勾选")} />
                : <input type="checkbox" checked={selected.has(item.id)} aria-label={t(item.label)} onChange={(e) => toggle([item.id], e.target.checked)} />}
            <i className={"ti " + KIND_ICON[item.kind]} aria-hidden="true" />
            <span className="leftovers-label">
              <b>{t(item.label)}</b>
              <small>{t(item.explain)}</small>
              <small className="leftovers-path" title={item.path}>{item.path}</small>
              {item.blocked && <small className="warn">{t(item.blocked)}</small>}
            </span>
            <span className="footprint-size">{bytes(item.bytes)}</span>
          </div>)}
        </div>}
      </section>;
    })}

    {cleaning && <LeftoversDialog ids={cleaning} onClose={(changed) => { setCleaning(null); if (changed) void load(true); }} />}
  </div>;
}
