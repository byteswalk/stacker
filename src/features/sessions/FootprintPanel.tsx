import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { formatSpaceBytes as bytes } from "../space-analysis/components/SpaceOverview";
import { migrationStatus, scanFootprint } from "./api";
import { LocationActions, locationText } from "./DataLocations";
import { useBusyRead } from "../../ui";
import { FootprintDialog } from "./FootprintDialog";
import { AGENT_LABEL } from "./sessionsView";
import { errorMessage, type FootprintItem, type FootprintKind, type FootprintReport, type LocationStatus } from "./types";

const SECTIONS: { kind: FootprintKind; title: string; hint: string }[] = [
  { kind: "sessions", title: "会话记录", hint: "按会话删除，删除前可精简导出" },
  { kind: "reclaimable", title: "可安全清理", hint: "能确认已无用，删除后智能体按需重建" },
  { kind: "review", title: "需你判断", hint: "智能体的工作产物，确认不再需要后再删除" },
  { kind: "keep", title: "保留", hint: "智能体运行所需，不提供删除" },
];

// Survives tab switches; a scan takes several seconds.
let cachedReport: FootprintReport | null = null;

/** Reclaimable and unblocked items start selected; review items never do. */
export function defaultSelection(report: FootprintReport): string[] {
  return report.agents.flatMap((a) => a.items).filter((i) => i.kind === "reclaimable" && !i.blocked).map((i) => i.id);
}

export function FootprintPanel({ onShowSessions }: { onShowSessions: (agent: string) => void }) {
  const { tr: t } = useI18n();
  const read = useBusyRead();
  const [report, setReport] = useState<FootprintReport | null>(cachedReport);
  const [selected, setSelected] = useState<string[]>(() => (cachedReport ? defaultSelection(cachedReport) : []));
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const [cleaning, setCleaning] = useState<string[] | null>(null);
  const request = useRef(0);
  const [locations, setLocations] = useState<LocationStatus[]>([]);

  const loadLocations = useCallback(() => {
    migrationStatus().then((list) => setLocations(list ?? [])).catch((e) => setError(errorMessage(e)));
  }, []);
  useEffect(() => { loadLocations(); }, [loadLocations]);

  const load = useCallback(async (refresh: boolean) => {
    const current = ++request.current;
    setLoading(true); setError("");
    try {
      const next = await read("正在统计智能体数据", () => scanFootprint(refresh), "逐个目录计算占用，约需 10 秒。");
      if (current !== request.current) return;
      cachedReport = next; setReport(next); setSelected(defaultSelection(next));
    } catch (e) {
      if (current === request.current) setError(errorMessage(e));
    } finally {
      if (current === request.current) setLoading(false);
    }
  }, [read]);

  useEffect(() => { if (!cachedReport) void load(false); }, [load]);

  const items = report?.agents.flatMap((a) => a.items) ?? [];
  const selectedBytes = items.filter((i) => selected.includes(i.id)).reduce((sum, i) => sum + i.bytes, 0);
  const toggle = (id: string) => setSelected((old) => old.includes(id) ? old.filter((x) => x !== id) : [...old, id]);

  const row = (item: FootprintItem) => {
    const selectable = (item.kind === "reclaimable" || item.kind === "review") && !item.blocked;
    const checked = selected.includes(item.id);
    return <div key={item.id} className={`footprint-row ${checked ? "selected" : ""} ${item.kind}`}>
      {selectable ? <input type="checkbox" checked={checked} aria-label={item.label} onChange={() => toggle(item.id)} /> : <span />}
      <button className="footprint-label" aria-expanded={open === item.id} onClick={() => setOpen(open === item.id ? null : item.id)}>
        <b>{t(item.label)}</b>
        <small className={item.kind === "review" && checked ? "warn" : ""}>{t(item.explain)}</small>
        {item.blocked && <small className="warn"><i className="ti ti-lock" /> {t(errorMessage(item.blocked))}</small>}
      </button>
      {item.kind === "sessions"
        ? <button className="gh sm" onClick={() => onShowSessions(item.agent)}>{t("查看最大的会话")}</button>
        : <span />}
      <span className="footprint-size">{bytes(item.bytes)}</span>
      {open === item.id && <ul className="footprint-paths">{item.paths.slice(0, 50).map((p) => <li key={p}><code>{p}</code></li>)}
        {item.paths.length > 50 && <li>{t("另有")} {item.paths.length - 50} {t("项")}</li>}</ul>}
    </div>;
  };

  return <div className="footprint">
    <div className="footprint-summary">
      <div>
        <span>{t("智能体数据共")}</span><b>{report ? bytes(report.total) : "—"}</b>
        <span>{t("其中可安全清理")}</span><b className="accent">{report ? bytes(report.reclaimable) : "—"}</b>
      </div>
      <div className="session-actions">
        <button className="gh sm" disabled={loading} onClick={() => void load(true)}><i className={`ti ${loading ? "ti-loader spin" : "ti-refresh"}`} />{t(loading ? "正在统计…" : "重新统计")}</button>
        <button className="pr sm" disabled={loading || !selected.length} onClick={() => setCleaning([...selected])}><i className="ti ti-trash" />{t("清理所选")} {bytes(selectedBytes)}</button>
      </div>
    </div>
    {error && <div role="alert" className="session-error">{t(error)}</div>}
    {!report && loading && <div className="session-empty"><i className="ti ti-loader spin" /><b>{t("正在统计智能体数据，约需 10 秒…")}</b></div>}
    {report?.agents.map((agent) => <section key={agent.agent} className="footprint-agent">
      <header><b>{AGENT_LABEL[agent.agent]}</b><span>{bytes(agent.total)}</span>
        {(() => {
          const loc = locations.find((l) => l.agent === agent.agent);
          if (!loc) return null;
          const shown = locationText(loc, t);
          return <div className="footprint-location">
            <code title={shown}>{shown}</code>
            <LocationActions loc={loc} onChanged={() => { loadLocations(); void load(true); }} />
          </div>;
        })()}
      </header>
      {SECTIONS.map(({ kind, title, hint }) => {
        const list = agent.items.filter((i) => i.kind === kind);
        if (!list.length) return null;
        return <div key={kind} className={`footprint-section ${kind}`}>
          <div className="footprint-section-head"><b>{t(title)}</b><small>{t(hint)}</small><span>{bytes(list.reduce((s, i) => s + i.bytes, 0))}</span></div>
          {list.map(row)}
        </div>;
      })}
    </section>)}
    {!!report?.warnings.length && <p className="session-note">{report.warnings.length} {t("个目录无法读取，未计入统计。")}</p>}
    {cleaning && <FootprintDialog ids={cleaning} onClose={(changed) => { setCleaning(null); if (changed) void load(true); }} />}
  </div>;
}
