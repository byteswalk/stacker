import { useCallback, useEffect, useState } from "react";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { ConfirmModal, useToast } from "../../ui";
import { Select } from "../../Select";
import { AiAskModal, askAi } from "../ai/AiAsk";
import { INTERNAL, LogDetail, parseDetail, statusMeaning, type LogRow } from "./LogDetail";

type LogPage = { items: LogRow[]; total: number; kept: number; agents: string[] };
type Query = { search: string; outcome: string; agent: string; since: number; offset: number; limit: number };

const PAGE = 50;
const EMPTY: Query = { search: "", outcome: "", agent: "", since: 0, offset: 0, limit: PAGE };

/** Retention the page offers; 0 means the log is only emptied by hand. */
export const RETENTIONS = [1, 7, 30, 90, 0];

export function retentionLabel(days: number): string {
  return days === 0 ? "一直保留" : `保留 ${days} 天`;
}

const RANGES: { value: number; label: string }[] = [
  { value: 0, label: "全部时间" },
  { value: 3600, label: "最近 1 小时" },
  { value: 86_400, label: "最近 24 小时" },
  { value: 7 * 86_400, label: "最近 7 天" },
];

/** What the API service was asked to do, with the filters and clean-up it needs to stay useful. */

export function GatewayLog({ enabled, retentionDays, onSettings }: {
  enabled: boolean;
  retentionDays: number;
  onSettings: (enabled: boolean, retentionDays: number) => void;
}) {
  const { tr: t } = useI18n();
  const toast = useToast();
  const [query, setQuery] = useState<Query>(EMPTY);
  const [range, setRange] = useState(0);
  const [explain, setExplain] = useState<LogRow | null>(null);
  const [opened, setOpened] = useState<LogRow | null>(null);
  const [page, setPage] = useState<LogPage | null>(null);
  const [picked, setPicked] = useState<number[]>([]);
  const [clearing, setClearing] = useState(false);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async (next: Query) => {
    setBusy(true);
    try { setPage(await invoke<LogPage>("gateway_log", { query: next })); }
    catch (e) { toast(String(e), "err"); }
    finally { setBusy(false); }
  }, [toast]);

  useEffect(() => { void load(query); }, [load, query]);

  function change(patch: Partial<Query>) {
    setPicked([]);
    setQuery((old) => ({ ...old, offset: 0, ...patch }));
  }

  async function removeRows(ids: number[]) {
    try {
      await invoke<number>("gateway_log_remove", { ids });
      setPicked([]);
      await load(query);
    } catch (e) { toast(String(e), "err"); }
  }

  async function clearMatching() {
    setClearing(false);
    try {
      const gone = await invoke<number>("gateway_log_clear", { query });
      toast(`${t("已清除")} ${gone} ${t("条记录")}`, "ok");
      setPicked([]);
      await load({ ...query, offset: 0 });
    } catch (e) { toast(String(e), "err"); }
  }

  const rows = page?.items ?? [];
  const filtered = !!(query.search.trim() || query.outcome || query.agent || query.since);
  const allPicked = rows.length > 0 && picked.length === rows.length;
  const pages = Math.max(1, Math.ceil((page?.total ?? 0) / PAGE));
  const current = Math.floor(query.offset / PAGE) + 1;

  return <div className="pxcard gw-log-card">
    <div className="pxsec">
      <i className="ti ti-list" /> {t("请求记录")}
      <span className="pxhint">{t("接口服务的请求和 Stacker 自己的 AI 调用都记在这里；只记录接口、模型、耗时和结果，不记录内容")}</span>
    </div>

    <div className="gw-log-bar">
      <label className="gw-search">
        <i className="ti ti-search" />
        <input value={query.search} placeholder={t("搜索接口或模型…")} onChange={(e) => change({ search: e.target.value })} />
      </label>
      <Select value={query.outcome} width={110} onChange={(v) => change({ outcome: v })} options={[
        { value: "", label: t("全部结果") },
        { value: "ok", label: t("成功") },
        { value: "error", label: t("失败") },
      ]} />
      <Select value={query.agent} width={130} onChange={(v) => change({ agent: v })} options={[
        { value: "", label: t("全部智能体") },
        ...(page?.agents ?? []).map((a) => ({ value: a, label: a })),
      ]} />
      <Select value={String(range)} width={130} onChange={(v) => { setRange(Number(v)); change({ since: Number(v) ? Math.floor(Date.now() / 1000) - Number(v) : 0 }); }}
        options={RANGES.map((r) => ({ value: String(r.value), label: t(r.label) }))} />
      <span className="s dim">{page ? `${page.total} / ${page.kept}` : "—"} {t("条")}</span>
    </div>

    {!rows.length ? <p className="proxy-note">{t(filtered ? "没有匹配的记录。" : "还没有请求。")}</p> : <>
      <div className="gw-log">
        <div className="head">
          <label className="ck"><input type="checkbox" checked={allPicked} onChange={(e) => setPicked(e.target.checked ? rows.map((r) => r.id) : [])} /></label>
          <span>{t("时间")}</span><span>{t("接口")}</span><span>{t("模型")}</span><span>{t("状态")}</span><span>{t("耗时")}</span><span />
        </div>
        {rows.map((r) => <div key={r.id} className={"row" + (picked.includes(r.id) ? " on" : "")} onClick={() => setOpened(r)} title={t("查看这条请求的详情")}>
          <label className="ck" onClick={(event) => event.stopPropagation()}><input type="checkbox" checked={picked.includes(r.id)} aria-label={String(r.at)}
            onChange={(e) => setPicked((old) => e.target.checked ? [...old, r.id] : old.filter((id) => id !== r.id))} /></label>
          <span>{new Date(r.at * 1000).toLocaleString()}</span>
          {r.endpoint === INTERNAL
            ? <span className="gw-log-internal" title={t("Stacker 自己的 AI 功能发起的调用，不经过接口服务")}><i className="ti ti-sparkles" /> {t("Stacker 内部")}</span>
            : <code>{r.endpoint}</code>}
          <span>{r.model || "—"}</span>
          <b className={r.status < 400 ? "ok" : "bad"} title={t(statusMeaning(r.status))}>{r.status}</b>
          <span>{(r.elapsedMs / 1000).toFixed(1)}s</span>
          <span className="gw-log-acts" onClick={(event) => event.stopPropagation()}>
            {r.status >= 400 && <button className="gh xs ai-btn" title={t("问问 AI 这条为什么失败")} onClick={() => setExplain(r)}><i className="ti ti-sparkles" /></button>}
            <button className="gh xs" title={t("删除这条记录")} onClick={() => void removeRows([r.id])}><i className="ti ti-trash" /></button>
          </span>
        </div>)}
      </div>
      <div className="gw-log-foot">
        <button className="gh sm danger" disabled={!picked.length} onClick={() => void removeRows(picked)}>
          <i className="ti ti-trash" /> {t("删除所选")}（{picked.length}）
        </button>
        <button className="gh sm" disabled={!rows.length} onClick={() => setClearing(true)}>
          <i className="ti ti-eraser" /> {t(filtered ? "清除筛选结果" : "清空记录")}
        </button>
        {pages > 1 && <span className="gw-log-pager">
          <button className="gh sm" disabled={query.offset === 0 || busy} onClick={() => setQuery((q) => ({ ...q, offset: Math.max(0, q.offset - PAGE) }))}><i className="ti ti-chevron-left" /></button>
          <span className="s dim">{current} / {pages}</span>
          <button className="gh sm" disabled={current >= pages || busy} onClick={() => setQuery((q) => ({ ...q, offset: q.offset + PAGE }))}><i className="ti ti-chevron-right" /></button>
        </span>}
      </div>
    </>}

    <div className="gw-log-policy">
      <label className="ck" title={t("关闭后不再记录任何请求")}>
        <input type="checkbox" checked={enabled} onChange={(e) => onSettings(e.target.checked, retentionDays)} /> {t("记录请求")}
      </label>
      <Select value={String(retentionDays)} width={130} onChange={(v) => onSettings(enabled, Number(v))}
        options={RETENTIONS.map((d) => ({ value: String(d), label: t(retentionLabel(d)) }))} />
      <span className="s dim">{t("超过保留期的记录会在下次请求时自动删除")}</span>
    </div>

    {clearing && <ConfirmModal title={t(filtered ? "清除筛选结果" : "清空请求记录")} icon="ti-eraser" danger
      message={t(filtered
        ? "将删除当前筛选条件匹配的全部记录，其余记录保留。"
        : "将删除全部请求记录。记录里只有接口、模型、耗时和状态，删除不影响任何会话。")}
      confirmLabel={t("删除")}
      onConfirm={() => void clearMatching()}
      onClose={() => setClearing(false)} />}
    {opened && !explain && <LogDetail row={opened} onClose={() => setOpened(null)} onDiagnose={() => setExplain(opened)} />}
    {explain && <AiAskModal title={t("这条请求为什么失败")} sub={`${explain.endpoint} · ${explain.model || "—"} · ${explain.status}`}
      note={t("只把这条记录（接口、模型、状态码、耗时、调用方式和错误信息）发给 AI，不含请求内容。")}
      run={() => {
        const detail = parseDetail(explain.detail);
        return askAi("gateway_error", { entry: { endpoint: explain.endpoint, model: explain.model, status: explain.status, elapsedMs: explain.elapsedMs, error: detail.error, stream: detail.stream, effort: detail.effort, client: detail.userAgent } });
      }}
      onClose={() => setExplain(null)} />}
  </div>;
}
