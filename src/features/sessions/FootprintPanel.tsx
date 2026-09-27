import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { formatSpaceBytes as bytes } from "../space-analysis/components/SpaceOverview";
import { migrationStatus, scanFootprint } from "./api";
import { LocationActions, locationText } from "./DataLocations";
import { useBusyRead } from "../../ui";
import { FootprintDialog } from "./FootprintDialog";
import { AGENT_LABEL } from "./sessionsView";
import { invoke } from "../../invoke";
import { errorMessage, type FootprintItem, type FootprintKind, type FootprintReport, type LocationStatus } from "./types";

// Survives tab switches; a scan takes several seconds.
let cachedReport: FootprintReport | null = null;

/** Reclaimable and unblocked items start selected; review items never do. */
export function defaultSelection(report: FootprintReport): string[] {
  return report.agents.flatMap((a) => a.items).filter((i) => i.kind === "reclaimable" && !i.blocked).map((i) => i.id);
}

type Slice = { kind: FootprintKind; label: string; color: string; bytes: number };

/** The four kinds as one bar: what the space is, before what can be done about it. */
export function slices(items: FootprintItem[]): Slice[] {
  const sum = (kind: FootprintKind) => items.filter((i) => i.kind === kind).reduce((n, i) => n + i.bytes, 0);
  return ([
    { kind: "sessions", label: "会话记录", color: "#5b8def" },
    { kind: "review", label: "需你判断", color: "#e6b450" },
    { kind: "reclaimable", label: "可安全清理", color: "#6bcf86" },
    { kind: "keep", label: "运行必需", color: "#6b7380" },
  ] as Slice[]).map((s) => ({ ...s, bytes: sum(s.kind) })).filter((s) => s.bytes > 0);
}

const REVIEW_ICON: { match: RegExp; icon: string }[] = [
  { match: /日志|log/i, icon: "ti-database" },
  { match: /编译|target|build/i, icon: "ti-tools" },
  { match: /图片|image|截图/i, icon: "ti-photo" },
  { match: /附件|attachment/i, icon: "ti-paperclip" },
  { match: /目录|工作/i, icon: "ti-folder" },
];

function reviewIcon(label: string): string {
  return REVIEW_ICON.find((r) => r.match.test(label))?.icon ?? "ti-help-circle";
}

export function FootprintPanel({ onShowSessions }: { onShowSessions: (agent: string) => void }) {
  const { tr: t } = useI18n();
  const read = useBusyRead();
  const [report, setReport] = useState<FootprintReport | null>(cachedReport);
  const [selected, setSelected] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const [details, setDetails] = useState<string | null>(null);
  const [cleaning, setCleaning] = useState<string[] | null>(null);
  const [shown, setShown] = useState<string | null>(null);
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
      cachedReport = next; setReport(next); setSelected([]);
    } catch (e) {
      if (current === request.current) setError(errorMessage(e));
    } finally {
      if (current === request.current) setLoading(false);
    }
  }, [read]);

  useEffect(() => { if (!cachedReport) void load(false); }, [load]);

  const toggle = (id: string) => setSelected((old) => old.includes(id) ? old.filter((x) => x !== id) : [...old, id]);

  /** Opens the folder an item lives in, so the user can look before deciding. */
  const openFolder = (item: FootprintItem) => {
    const path = item.paths[0];
    if (!path) return;
    invoke("space_open_directory", { path }).catch((e) => setError(errorMessage(e)));
  };

  const paths = (item: FootprintItem) => open === item.id && <ul className="footprint-paths">
    {item.paths.slice(0, 50).map((p) => <li key={p}><code>{p}</code></li>)}
    {item.paths.length > 50 && <li>{t("另有")} {item.paths.length - 50} {t("项")}</li>}
  </ul>;

  return <div className="footprint">
    <div className="footprint-summary">
      <div>
        <span>{t("智能体数据共")}</span><b>{report ? bytes(report.total) : "—"}</b>
        <span>{t("其中可安全清理")}</span><b className="accent">{report ? bytes(report.reclaimable) : "—"}</b>
      </div>
      <div className="session-actions">
        <button className="gh sm" disabled={loading} onClick={() => void load(true)}><i className={`ti ${loading ? "ti-loader spin" : "ti-refresh"}`} />{t(loading ? "正在统计…" : "重新统计")}</button>
      </div>
    </div>
    {error && <div role="alert" className="session-error">{t(error)}</div>}
    {!report && loading && <div className="session-empty"><i className="ti ti-loader spin" /><b>{t("正在统计智能体数据，约需 10 秒…")}</b></div>}

    {!!report?.agents.length && <div className="fp-tiles">
      {report.agents.map((agent) => {
        const parts = slices(agent.items);
        const safe = agent.items.filter((i) => i.kind === "reclaimable" && !i.blocked).reduce((n, i) => n + i.bytes, 0);
        return <button type="button" key={agent.agent} className={"fp-tile" + (shown === agent.agent ? " on" : "")}
          onClick={() => setShown(shown === agent.agent ? null : agent.agent)}>
          <span className="fp-tile-head"><b>{AGENT_LABEL[agent.agent]}</b><span>{bytes(agent.total)}</span></span>
          <span className="fp-bar">{parts.map((p) => <i key={p.kind} style={{ width: `${(p.bytes / Math.max(agent.total, 1)) * 100}%`, background: p.color }} />)}</span>
          <span className="fp-tile-foot">
            {safe > 0 ? <em className="green">{t("可清理")} {bytes(safe)}</em> : <em>{t("无可清理")}</em>}
            <i className={"ti " + (shown === agent.agent ? "ti-chevron-up" : "ti-chevron-down")} />
          </span>
        </button>;
      })}
    </div>}

    {report?.agents.filter((agent) => shown === agent.agent).map((agent) => {
      const parts = slices(agent.items);
      const reclaimable = agent.items.filter((i) => i.kind === "reclaimable" && !i.blocked);
      const blocked = agent.items.filter((i) => i.kind === "reclaimable" && i.blocked);
      const reclaimableBytes = reclaimable.reduce((n, i) => n + i.bytes, 0);
      const sessions = agent.items.filter((i) => i.kind === "sessions");
      const review = agent.items.filter((i) => i.kind === "review");
      const keep = agent.items.filter((i) => i.kind === "keep");
      const picked = review.filter((i) => selected.includes(i.id));
      const loc = locations.find((l) => l.agent === agent.agent);
      return <section key={agent.agent} className="fp-agent">
        <header>
          <b>{AGENT_LABEL[agent.agent]}</b>
          <span className="fp-total">{bytes(agent.total)}</span>
          {loc && <div className="footprint-location">
            <code title={locationText(loc, t)}>{locationText(loc, t)}</code>
            <LocationActions loc={loc} onChanged={() => { loadLocations(); void load(true); }} />
          </div>}
        </header>

        <div className="fp-legend">{parts.map((s) => <span key={s.kind}><em style={{ background: s.color }} />{t(s.label)} {bytes(s.bytes)}</span>)}</div>

        {!!reclaimable.length && <div className="fp-card">
          <span className="ic green"><i className="ti ti-recycle" /></span>
          <span className="tx">
            <b>{t("可安全清理")}</b>
            <span>{t("临时文件、缓存这类东西，删掉不影响任何会话，需要时智能体会自己重建。")}</span>
          </span>
          <span className="sz"><b>{bytes(reclaimableBytes)}</b><span>{reclaimable.length} {t("项")}</span></span>
          <button className="pr sm" disabled={loading} onClick={() => setCleaning(reclaimable.map((i) => i.id))}><i className="ti ti-trash" /> {t("一键清理")}</button>
          <button className="gh sm" onClick={() => setDetails(details === agent.agent ? null : agent.agent)}>{t(details === agent.agent ? "收起明细" : "明细")}</button>
        </div>}
        {details === agent.agent && <div className="fp-details">
          {reclaimable.concat(blocked).map((item) => <div key={item.id} className={item.blocked ? "blocked" : ""}>
            <button className="footprint-label" onClick={() => setOpen(open === item.id ? null : item.id)}>
              <b>{t(item.label)}</b><small>{t(item.explain)}</small>
              {item.blocked && <small className="warn"><i className="ti ti-lock" /> {t(errorMessage(item.blocked))}</small>}
            </button>
            <span className="footprint-size">{bytes(item.bytes)}</span>
            <button className="gh xs" title={`${t("打开所在目录")}：${item.paths[0] ?? ""}`} disabled={!item.paths.length} onClick={() => openFolder(item)}><i className="ti ti-folder-open" /></button>
            {paths(item)}
          </div>)}
        </div>}

        {sessions.map((item) => <div className="fp-card" key={item.id}>
          <span className="ic blue"><i className="ti ti-message" /></span>
          <span className="tx"><b>{t(item.label)}</b><span>{t(item.explain)}</span></span>
          <span className="sz"><b>{bytes(item.bytes)}</b><span>{Math.round((item.bytes / Math.max(agent.total, 1)) * 100)}%</span></span>
          <button className="gh sm" onClick={() => onShowSessions(item.agent)}>{t("查看最大的会话")}</button>
        </div>)}

        {!!review.length && <>
          <div className="fp-sec">
            <b>{t("需你判断")}</b>
            <span>{t("智能体的工作产物，确认不再需要后再删")} · {t("合计")} {bytes(review.reduce((n, i) => n + i.bytes, 0))}</span>
          </div>
          {review.map((item) => <div className={"fp-pick" + (selected.includes(item.id) ? " on" : "")} key={item.id}>
            {item.blocked
              ? <span className="fp-lock" title={t(errorMessage(item.blocked))}><i className="ti ti-lock" /></span>
              : <input type="checkbox" checked={selected.includes(item.id)} aria-label={t(item.label)} onChange={() => toggle(item.id)} />}
            <span className="ic amber"><i className={"ti " + reviewIcon(item.label)} /></span>
            <button className="footprint-label" onClick={() => setOpen(open === item.id ? null : item.id)}>
              <b>{t(item.label)}</b>
              <small>{t(item.explain)}</small>
              {item.blocked && <small className="warn">{t(errorMessage(item.blocked))}</small>}
            </button>
            <span className="footprint-size">{bytes(item.bytes)}</span>
            <button className="gh xs" title={`${t("打开所在目录")}：${item.paths[0] ?? ""}`} disabled={!item.paths.length} onClick={() => openFolder(item)}><i className="ti ti-folder-open" /></button>
            {paths(item)}
          </div>)}
          <div className="fp-pickbar">
            <button className="pr sm danger" disabled={!picked.length || loading} onClick={() => setCleaning(picked.map((i) => i.id))}>
              <i className="ti ti-trash" /> {t("删除所选")}（{picked.length} {t("项")}{picked.length ? ` · ${bytes(picked.reduce((n, i) => n + i.bytes, 0))}` : ""}）
            </button>
            <span className="s dim">{t("勾选后才能删除，删除前还会再确认一次")}</span>
          </div>
        </>}

        {!!keep.length && <p className="fp-keep">
          <i className="ti ti-lock" /> {t("运行必需")} {bytes(keep.reduce((n, i) => n + i.bytes, 0))}：{keep.map((i) => t(i.label)).join("、")}
        </p>}
      </section>;
    })}

    {!!report?.warnings.length && <p className="session-note">{report.warnings.length} {t("个目录无法读取，未计入统计。")}</p>}
    {cleaning && <FootprintDialog ids={cleaning} onClose={(changed) => { setCleaning(null); if (changed) void load(true); }} />}
  </div>;
}
