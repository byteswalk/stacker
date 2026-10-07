import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { useToast } from "../../ui";
import { listProjects, listSessions, setFavorite } from "./api";
import { DeleteDialog } from "./DeleteDialog";
import { TransferExport, TransferImport } from "./TransferDialogs";
import { DistillDialog } from "./DistillDialog";
import { DistillPanel } from "./DistillPanel";
import { FootprintPanel } from "./FootprintPanel";
import { ProjectList } from "./ProjectList";
import { SessionDetail } from "./SessionDetail";
import { SessionList } from "./SessionList";
import { currentSelection } from "./sessionsView";
import { SettingsPanel } from "./SettingsPanel";
import { SummaryDialog } from "./SummaryDialog";
import { EMPTY_PAGE, EMPTY_QUERY, errorMessage, type DistillSourceRef, type ProjectRow, type Session, type SessionPage, type SessionQuery } from "./types";
import { WebChatPanel } from "./WebChatPanel";
import "./sessions.css";

type Tab = "sessions" | "projects" | "web" | "distill" | "footprint" | "sources";
const TABS: [Tab, string, string][] = [["sessions", "会话", "ti-messages"], ["projects", "项目", "ti-folders"], ["web", "网页对话", "ti-world"], ["distill", "提炼", "ti-bulb"], ["footprint", "占用", "ti-chart-pie"], ["sources", "设置", "ti-settings"]];

// Survive page switches within one app session.
let lastTab: Tab = "sessions";
let lastQuery = EMPTY_QUERY;
let cachedPage = EMPTY_PAGE;
let cachedProjects: ProjectRow[] = [];

export function SessionCatalog({ onCleanup }: { onCleanup: () => void }) {
  const { tr: t } = useI18n();
  const toast = useToast();
  const [tab, setTab] = useState<Tab>(lastTab);
  const [query, setQuery] = useState<SessionQuery>(lastQuery);
  const [page, setPage] = useState<SessionPage>(cachedPage);
  const [projects, setProjects] = useState<ProjectRow[]>(cachedProjects);
  const [selected, setSelected] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [detail, setDetail] = useState<Session | null>(null);
  const [deleting, setDeleting] = useState<string[] | null>(null);
  const [migrating, setMigrating] = useState<string[] | null>(null);
  const [importing, setImporting] = useState(false);
  const [summarizing, setSummarizing] = useState<{ kind: "summary"; ids: string[] } | { kind: "handoff"; project: string } | null>(null);
  const request = useRef(0);
  const queryRef = useRef(query);
  const [webRefresh, setWebRefresh] = useState(0);
  const [distilling, setDistilling] = useState<DistillSourceRef[] | null>(null);
  const [distillRefresh, setDistillRefresh] = useState(0);

  useEffect(() => { lastTab = tab; }, [tab]);

  const load = useCallback(async () => {
    const generation = ++request.current;
    setLoading(true);
    try {
      // No dialog: the list says it is reading, and the result waits here if the user goes elsewhere.
      const [nextPage, nextProjects] = await Promise.all([listSessions(queryRef.current), listProjects()]);
      if (generation !== request.current) return;
      cachedPage = nextPage; cachedProjects = nextProjects;
      setPage(nextPage); setProjects(nextProjects);
      setSelected((old) => currentSelection(old, nextPage.ids));
      setError("");
    } catch (e) {
      if (generation === request.current) setError(errorMessage(e));
    } finally {
      if (generation === request.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    queryRef.current = query; lastQuery = query;
    const timer = window.setTimeout(() => void load(), 300);
    return () => clearTimeout(timer);
  }, [query, load]);

  function filter(patch: Partial<SessionQuery>) {
    setQuery((old) => ({ ...old, ...patch, offset: 0 }));
    setSelected([]);
  }

  async function favorite(ids: string[], value: boolean) {
    try { await setFavorite(ids, value); await load(); }
    catch (e) { toast(t(errorMessage(e)), "err"); }
  }

  const warnings = page.warnings.map((w) => {
    const [agent, code] = w.split(":");
    return `${agent === "codex" ? "Codex" : "Claude"}：${t(errorMessage(code))}`;
  });

  return <div className="session-catalog">
    <div className="session-heading">
      <div className="space-analysis-tabs" role="tablist">
        {TABS.map(([value, label, icon]) => <button key={value} role="tab" aria-selected={tab === value} className={tab === value ? "active" : ""} onClick={() => setTab(value)}><i className={`ti ${icon}`} />{t(label)}</button>)}
      </div>
      <div className="session-actions">
        <button className="gh sm" title={t("导入另一台电脑打出来的会话迁移包")} onClick={() => setImporting(true)}><i className="ti ti-transfer-in" />{t("导入迁移包")}</button>
        <button className="gh sm" onClick={onCleanup}><i className="ti ti-device-desktop-analytics" />{t("检查磁盘空间")}</button>
        <button className="gh sm" disabled={loading} onClick={() => { if (tab === "web") setWebRefresh((n) => n + 1); else if (tab === "distill") setDistillRefresh((n) => n + 1); else void load(); }}><i className={`ti ${loading ? "ti-loader spin" : "ti-refresh"}`} />{t("刷新")}</button>
      </div>
    </div>
    {warnings.map((w) => <div key={w} role="alert" className="session-error"><i className="ti ti-alert-triangle" />{w}</div>)}
    {error && <div role="alert" className="session-error">{t(error)}<button className="gh sm" onClick={() => void load()}><i className="ti ti-refresh" />{t("重试")}</button></div>}
    {tab === "sessions" && <SessionList page={page} query={query} projects={projects} loading={loading} selected={selected}
      onSelect={setSelected} onFilter={filter} onPage={(offset) => setQuery((q) => ({ ...q, offset }))}
      onOpen={setDetail} onFavorite={(ids, value) => void favorite(ids, value)} onDelete={() => setDeleting([...selected])} onSummarize={() => setSummarizing({ kind: "summary", ids: [...selected] })}
      onDistill={() => setDistilling(selected.map((id) => ({ kind: "session" as const, key: id })))}
      onMigrate={() => setMigrating([...selected])} />}
    {tab === "projects" && <ProjectList rows={projects} loading={loading} onPick={(project) => { filter({ project }); setTab("sessions"); }} onHandoff={(project) => setSummarizing({ kind: "handoff", project })} />}
    {tab === "web" && <WebChatPanel refresh={webRefresh} onDistill={(key) => setDistilling([{ kind: "web", key }])} />}
    {tab === "distill" && <DistillPanel refresh={distillRefresh} onNew={() => setDistilling([])} />}
    {tab === "footprint" && <FootprintPanel onShowSessions={(agent) => { filter({ agent, sort: "bytes" }); setTab("sessions"); }} />}
    {tab === "sources" && <SettingsPanel />}
    {detail && <SessionDetail session={detail} onClose={() => setDetail(null)} onSummarize={() => { setDetail(null); setSummarizing({ kind: "summary", ids: [detail.id] }); }} />}
    {summarizing && <SummaryDialog target={summarizing} onClose={(changed) => { setSummarizing(null); if (changed) void load(); }} />}
    {migrating && <TransferExport ids={migrating} onClose={() => setMigrating(null)} />}
    {importing && <TransferImport onClose={(changed) => { setImporting(false); if (changed) void load(); }} />}
    {deleting && <DeleteDialog ids={deleting} onClose={(changed) => { setDeleting(null); if (changed) { setSelected([]); void load(); } }} />}
    {distilling && <DistillDialog initial={distilling} onClose={(changed) => { setDistilling(null); if (changed) setDistillRefresh((n) => n + 1); }} />}
  </div>;
}
