import { useCallback, useEffect, useMemo, useState } from "react";
import { t } from "../../i18n";
import {
  addTag, bodyIsFresh, createFolder, deleteFolder, getBody, listAccounts, listConversations, listFolders,
  openDb, putBody, renameAccount, renameFolder, updateLocal, type Account, type Conversation, type Db, type Folder,
} from "../../lib/db";
import { brokenSitesIn, createBrokenSites, type BrokenSites } from "../../lib/brokenSites";
import { runDeleteJob } from "../../lib/deleteJob";
import { saveFile } from "../../lib/download";
import { exportFileName, toMarkdown, type ExportMode } from "../../lib/markdown";
import { createPacer, withPacing } from "../../lib/pacer";
import { refreshIndex } from "../../lib/refresh";
import { allTags, applyFilter, bodyText, EMPTY_FILTER, type Filter } from "../../lib/search";
import { createSiteApi } from "../../lib/siteClient";
import { SiteError, type SiteId } from "../../shared/types";
import { conversationUrl, SITES } from "../../sites/registry";
import { errorText } from "../errors";
import { ConversationList } from "./ConversationList";
import { DeleteDialog } from "./DeleteDialog";
import { Detail } from "./Detail";
import { Filters } from "./Filters";

const api = createSiteApi();
const brokenStore = createBrokenSites();
const urlOf = (c: Conversation) => conversationUrl(c.site, c.id);

export function App() {
  const [db, setDb] = useState<Db | null>(null);
  const [convs, setConvs] = useState<Conversation[]>([]);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [bodies, setBodies] = useState(new Map<string, string>());
  const [filter, setFilter] = useState<Filter>(EMPTY_FILTER);
  const [selected, setSelected] = useState(new Set<string>());
  const [active, setActive] = useState<Conversation | null>(null);
  const [busy, setBusy] = useState("");
  const [message, setMessage] = useState<{ text: string; kind: "info" | "error" } | null>(null);
  const [deleting, setDeleting] = useState<Conversation[] | null>(null);
  const [current, setCurrent] = useState(new Set<string>());
  const [siteErrors, setSiteErrors] = useState<Partial<Record<SiteId, string>>>({});
  const [broken, setBroken] = useState<BrokenSites>({});

  const reload = useCallback(async (d: Db) => {
    const [c, a, f] = await Promise.all([listConversations(d), listAccounts(d), listFolders(d)]);
    setConvs(c); setAccounts(a); setFolders(f);
    setActive((old) => (old ? c.find((x) => x.key === old.key) ?? null : null));
  }, []);
  useEffect(() => { void openDb().then(async (d) => { setDb(d); await reload(d); }); }, [reload]);
  useEffect(() => { void brokenStore.all().then(setBroken); }, []);

  useEffect(() => {
    if (!db || !filter.inBody) return;
    void (async () => {
      const map = new Map<string, string>();
      for (const c of convs) if (c.bodyFetchedAt !== null) { const b = await getBody(db, c.key); if (b) map.set(c.key, bodyText(b)); }
      setBodies(map);
    })();
  }, [db, convs, filter.inBody]);

  const shown = useMemo(() => applyFilter(convs, filter, bodies), [convs, filter, bodies]);
  const aliasOf = useCallback((key: string) => accounts.find((a) => a.key === key)?.alias ?? key, [accounts]);
  const chosen = convs.filter((c) => selected.has(c.key));

  async function guarded(label: string, task: () => Promise<void>) {
    setBusy(label); setMessage(null);
    try { await task(); } catch (e) { setMessage({ text: errorText(e), kind: "error" }); } finally { setBusy(""); if (db) await reload(db); }
  }

  const refresh = (site: SiteId) => guarded(t("正在刷新列表"), async () => {
    let r;
    try {
      r = await refreshIndex(api, db!, site, createPacer(), (n) => setBusy(`${t("正在刷新列表")} ${n}`));
    } catch (e) {
      if (e instanceof SiteError && e.code === "E_BROKEN") setBroken(await brokenStore.mark([site], Date.now()));
      throw e;
    }
    setBroken(await brokenStore.clear(site));
    setCurrent((old) => new Set([...old, r.account.key]));
    setMessage({ text: `${SITES[site].label}：${t("共")} ${r.total}，${t("新增")} ${r.added}，${t("已删除")} ${r.removed}`, kind: "info" });
  });

  const readBody = (c: Conversation) => guarded(t("正在读取正文"), async () => {
    await putBody(db!, c.key, await api.read(c.site, c.id), Date.now());
  });

  const exportChosen = (mode: ExportMode) => guarded(t("正在导出"), async () => {
    const pacer = createPacer();
    for (const c of chosen) {
      let body = bodyIsFresh(c) ? await getBody(db!, c.key) : undefined;
      if (!body) {
        const fresh = await withPacing(pacer, () => api.read(c.site, c.id));
        await putBody(db!, c.key, fresh, Date.now());
        body = { ...fresh, key: c.key };
      }
      await saveFile(exportFileName(c, "md"), toMarkdown(c, aliasOf(c.account), body, mode, urlOf(c)), "text/markdown");
      if (mode === "full") await saveFile(exportFileName(c, "json"), JSON.stringify(body, null, 2), "application/json");
    }
    setMessage({ text: `${t("已导出")} ${chosen.length} ${t("条到下载目录的「Stacker 网页对话」文件夹")}`, kind: "info" });
  });

  async function openDelete() {
    const sites = [...new Set(chosen.map((c) => c.site))];
    const signedIn = new Set<string>();
    const errors: Partial<Record<SiteId, string>> = {};
    const stillBroken = await brokenStore.all();
    setBroken(stillBroken);
    for (const site of sites) {
      if (stillBroken[site]) { errors[site] = t("接口已变化，请先刷新该站点"); continue; }
      try { const a = await api.account(site); signedIn.add(`${site}:${a.remoteId}`); } catch (e) { errors[site] = errorText(e); }
    }
    setCurrent(signedIn);
    setSiteErrors(errors);
    setDeleting(chosen);
  }

  if (!db) return <p style={{ padding: 16 }}>{t("正在打开…")}</p>;
  const siteAccounts = accounts.filter((a) => !filter.site || a.site === filter.site);
  return <div className="layout">
    <div className="top">
      <b>{t("Stacker 网页对话")}</b>
      <select value={filter.site} onChange={(e) => setFilter({ ...filter, site: e.target.value as SiteId | "", account: "" })}>
        <option value="">{t("全部站点")}</option>
        {(Object.keys(SITES) as SiteId[]).map((s) => <option key={s} value={s}>{SITES[s].label}</option>)}
      </select>
      <select value={filter.account} onChange={(e) => setFilter({ ...filter, account: e.target.value })}>
        <option value="">{t("全部账号")}</option>
        {siteAccounts.map((a) => <option key={a.key} value={a.key}>{a.alias}</option>)}
      </select>
      {filter.account && <button onClick={() => { const alias = prompt(t("账号备注名"), aliasOf(filter.account)); if (alias) void renameAccount(db, filter.account, alias).then(() => reload(db)); }}>{t("改备注名")}</button>}
      {(Object.keys(SITES) as SiteId[]).map((s) => <button key={s} disabled={!!busy} onClick={() => void refresh(s)}>{t("刷新")} {SITES[s].label}</button>)}
      {(Object.keys(SITES) as SiteId[]).filter((s) => broken[s]).map((s) => <span key={s} className="err">{SITES[s].label}：{t("接口已变化")}</span>)}
      {(Object.keys(SITES) as SiteId[]).map((s) => <a key={s} href={SITES[s].origin} target="_blank" rel="noreferrer">{t("打开")} {SITES[s].label}</a>)}
      {busy && <span className="mut">{busy}…</span>}
      {message && <span className={message.kind === "error" ? "err" : "mut"}>{message.text}</span>}
    </div>
    <div className="side">
      <div><button onClick={() => setFilter({ ...filter, folder: "" })}>{t("全部")}</button></div>
      <div><button onClick={() => setFilter({ ...filter, folder: "none" })}>{t("未归入")}</button></div>
      {folders.map((f) => <div key={f.id} className="row">
        <button onClick={() => setFilter({ ...filter, folder: f.id })}>{f.name}</button>
        <button aria-label={t("重命名")} onClick={() => { const name = prompt(t("文件夹名称"), f.name); if (name) void renameFolder(db, f.id, name).then(() => reload(db)); }}>✎</button>
        <button aria-label={t("删除文件夹")} onClick={() => { if (confirm(t("删除文件夹？其中的对话不会被删除。"))) void deleteFolder(db, f.id).then(() => reload(db)); }}>×</button>
      </div>)}
      <button onClick={() => { const name = prompt(t("文件夹名称")); if (name) void createFolder(db, name, Date.now()).then(() => reload(db)); }}>＋ {t("新建文件夹")}</button>
      <h4>{t("标签")}</h4>
      <div><button onClick={() => setFilter({ ...filter, tag: "" })}>{t("全部标签")}</button></div>
      {allTags(convs).map((tag) => <div key={tag}><button className={filter.tag === tag ? "primary" : ""} onClick={() => setFilter({ ...filter, tag })}>{tag}</button></div>)}
    </div>
    <div className="main">
      <Filters value={filter} onChange={setFilter} tags={allTags(convs)} />
      {selected.size > 0 && <div className="row" style={{ marginBottom: 6 }}>
        <select defaultValue="" onChange={(e) => { if (e.target.value) void updateLocal(db, [...selected], { folderId: e.target.value === "none" ? null : e.target.value }).then(() => reload(db)); e.target.value = ""; }}>
          <option value="">{t("移到文件夹…")}</option>
          <option value="none">{t("未归入")}</option>
          {folders.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
        </select>
        <button onClick={() => { const tag = prompt(t("标签")); if (tag) void addTag(db, [...selected], tag).then(() => reload(db)); }}>{t("加标签")}</button>
        <button onClick={() => void updateLocal(db, [...selected], { favorite: true }).then(() => reload(db))}>{t("收藏")}</button>
        <button disabled={!!busy} onClick={() => void exportChosen("slim")}>{t("导出精简版")}</button>
        <button disabled={!!busy} onClick={() => void exportChosen("full")}>{t("导出完整版")}</button>
        <button className="danger" disabled={!!busy} onClick={() => void openDelete()}>{t("删除…")}</button>
      </div>}
      {shown.length === 0 ? <p className="mut">{t("没有对话。先打开并登录 ChatGPT 或 Claude，再点「刷新」。")}</p>
        : <ConversationList items={shown} aliasOf={aliasOf} selected={selected} active={active?.key ?? null} onOpen={setActive}
          onSelect={(keys, on) => setSelected((old) => { const next = new Set(old); keys.forEach((k) => (on ? next.add(k) : next.delete(k))); return next; })} />}
    </div>
    <div className="detail">
      {active ? <Detail db={db} conv={active} folders={folders} reading={!!busy} onRead={(c) => void readBody(c)} onChanged={() => void reload(db)} />
        : <p className="mut">{t("选择一条对话查看详情")}</p>}
    </div>
    {deleting && <DeleteDialog items={deleting} currentAccounts={current} aliasOf={aliasOf} siteErrors={siteErrors}
      onRun={async (runItems, mode, signal, onProgress) => {
        const results = await runDeleteJob(runItems, mode, { api, db, pacer: createPacer(), now: Date.now, save: saveFile, aliasOf }, signal, onProgress);
        const newlyBroken = brokenSitesIn(runItems, results);
        if (newlyBroken.length) setBroken(await brokenStore.mark(newlyBroken, Date.now()));
        return results;
      }}
      onClose={(changed) => { setDeleting(null); if (changed) { setSelected(new Set()); void reload(db); } }} />}
  </div>;
}
