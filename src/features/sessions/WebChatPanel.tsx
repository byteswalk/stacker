import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { Select } from "../../Select";
import { useBusyRead } from "../../ui";
import { listWebChats } from "./api";
import { WebChatDetail } from "./WebChatDetail";
import { EMPTY_WEB_QUERY, errorMessage, WEB_PAGE_SIZE, webSiteLabel, type WebChat, type WebPage, type WebQuery } from "./types";

// Survives page switches within one app session, like the sessions tab.
let lastWebQuery = EMPTY_WEB_QUERY;

/** 网页对话: conversations synced from the browser extension. */
export function WebChatPanel({ refresh, onDistill }: { refresh: number; onDistill?: (key: string) => void }) {
  const { tr: t, locale } = useI18n();
  const read = useBusyRead();
  const [query, setQuery] = useState<WebQuery>(lastWebQuery);
  const [page, setPage] = useState<WebPage | null>(null);
  const [error, setError] = useState("");
  const [open, setOpen] = useState<WebChat | null>(null);
  const request = useRef(0);

  const load = useCallback(async (q: WebQuery) => {
    const generation = ++request.current;
    try {
      const next = await read("正在读取网页对话", () => listWebChats(q));
      if (generation === request.current) { setPage(next); setError(""); }
    } catch (e) {
      if (generation === request.current) setError(errorMessage(e));
    }
  }, [read]);

  useEffect(() => {
    lastWebQuery = query;
    const timer = window.setTimeout(() => void load(query), 300);
    return () => clearTimeout(timer);
  }, [query, load, refresh]);

  const filter = (patch: Partial<WebQuery>) => setQuery((old) => ({ ...old, ...patch, offset: 0 }));
  const accounts = page?.accounts ?? [];
  const sites = [...new Set(accounts.map((a) => a.site))];
  const siteAccounts = accounts.filter((a) => !query.site || a.site === query.site);

  return <>
    <div className="session-filters">
      <label className="session-search"><i className="ti ti-search" /><input value={query.search} aria-label={t("搜索网页对话")} placeholder={t("搜索标题、备注、标签或摘要")} onChange={(e) => filter({ search: e.target.value })} /></label>
      <label className="session-check"><input type="checkbox" checked={query.fullText} onChange={(e) => filter({ fullText: e.target.checked })} />{t("搜索正文")}</label>
      <Select value={query.site} onChange={(site) => filter({ site, account: "" })} options={[{ value: "", label: t("全部站点") }, ...sites.map((s) => ({ value: s, label: webSiteLabel(s) }))]} />
      <Select value={query.account} onChange={(account) => filter({ account })} options={[{ value: "", label: t("全部账号") }, ...siteAccounts.map((a) => ({ value: a.key, label: `${webSiteLabel(a.site)} · ${a.name}` }))]} />
    </div>
    {error && <div role="alert" className="session-error">{t(error)}</div>}
    {page && page.total === 0 && <div className="session-empty">
      <i className="ti ti-world" />
      <b>{t("还没有网页对话")}</b>
      <span>{t("在「设置 → 浏览器插件」连接插件后，插件里的对话会同步到这里。")}</span>
    </div>}
    {page && page.total > 0 && <div className="session-list">
      {page.items.map((c) => <div key={c.key} className="session-item"><div className="web-chat-row">
        <button className="session-title" onClick={() => setOpen(c)}>
          <b>{c.favorite && <i className="ti ti-star" />}{c.title || t("（无标题）")}</b>
          <span>{[webSiteLabel(c.site), c.accountName, c.folder, c.tags.join("、")].filter(Boolean).join(" · ")}</span>
        </button>
        <div className="session-tags">
          {c.removedAt !== null && <span className="session-tag">{t("网站上已删除")}</span>}
          {c.bodyFetchedAt === null ? <span className="session-tag">{t("未存正文")}</span> : c.bodyStale ? <span className="session-tag">{t("正文不是最新")}</span> : null}
          {c.summary && <span className="session-tag">{t("有摘要")}</span>}
        </div>
        <div className="session-time">{new Date(c.updatedAt).toLocaleDateString(locale)}</div>
      </div></div>)}
    </div>}
    {page && page.total > WEB_PAGE_SIZE && <div className="session-pagination">
      <button className="gh sm" disabled={!query.offset} onClick={() => setQuery((q) => ({ ...q, offset: Math.max(0, q.offset - WEB_PAGE_SIZE) }))}>{t("上一页")}</button>
      <span>{query.offset + 1}–{Math.min(page.total, query.offset + WEB_PAGE_SIZE)} / {page.total}</span>
      <button className="gh sm" disabled={query.offset + WEB_PAGE_SIZE >= page.total} onClick={() => setQuery((q) => ({ ...q, offset: q.offset + WEB_PAGE_SIZE }))}>{t("下一页")}</button>
    </div>}
    {open && <WebChatDetail chat={open} onDistill={onDistill} onClose={(changed) => { setOpen(null); if (changed) void load(query); }} />}
  </>;
}
