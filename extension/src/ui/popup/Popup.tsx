import { useCallback, useEffect, useState } from "react";
import { t } from "../../i18n";
import { addTag, conversationKey, deleteExcerpt, getConversation, listExcerpts, listFolders, openDb, updateLocal, type Conversation, type Db, type Excerpt, type Folder } from "../../lib/db";
import { LAST_TAB_KEY, type SiteTab } from "../../lib/lastTab";
import { conversationIdOfUrl, SITES } from "../../sites/registry";

export function Popup() {
  const [db, setDb] = useState<Db | null>(null);
  const [tab, setTab] = useState<SiteTab | null>(null);
  const [conv, setConv] = useState<Conversation | null>(null);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [excerpts, setExcerpts] = useState<Excerpt[]>([]);
  const [tag, setTag] = useState("");

  useEffect(() => {
    void openDb().then(setDb);
    void chrome.storage.session.get(LAST_TAB_KEY).then((v) => setTab((v[LAST_TAB_KEY] as SiteTab) ?? null));
    const onChange = (changes: Record<string, chrome.storage.StorageChange>) => { if (changes[LAST_TAB_KEY]) setTab(changes[LAST_TAB_KEY].newValue as SiteTab); };
    chrome.storage.session.onChanged.addListener(onChange);
    return () => chrome.storage.session.onChanged.removeListener(onChange);
  }, []);

  const id = tab ? conversationIdOfUrl(tab.site, tab.url) : null;
  const load = useCallback(async () => {
    if (!db || !tab || !id) return;
    setConv((await getConversation(db, conversationKey(tab.site, id))) ?? null);
    setFolders(await listFolders(db));
    setExcerpts(await listExcerpts(db, tab.site, id));
  }, [db, tab, id]);
  useEffect(() => { void load(); }, [load]);

  if (!tab || !id) return <p style={{ padding: 12 }}>{t("在 ChatGPT 或 Claude 打开一条对话后，这里会显示它。")}</p>;
  const save = async (patch: Parameters<typeof updateLocal>[2]) => { if (db && conv) { await updateLocal(db, [conv.key], patch); await load(); } };
  return <div style={{ padding: 12 }}>
    <div className="mut">{SITES[tab.site].label}</div>
    <h3>{conv?.title || tab.title}</h3>
    {!conv && <p className="warn">{t("这条对话还不在列表里：请在管理页刷新该站点。")}</p>}
    {conv && db && <>
      <div className="row">
        <label><input type="checkbox" checked={conv.favorite} onChange={(e) => void save({ favorite: e.target.checked })} /> {t("收藏")}</label>
        <select value={conv.folderId ?? ""} onChange={(e) => void save({ folderId: e.target.value || null })}>
          <option value="">{t("未归入文件夹")}</option>
          {folders.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
        </select>
      </div>
      <div className="row" style={{ marginTop: 6 }}>
        {conv.tags.map((x) => <span key={x} className="chip">{x}</span>)}
        <input placeholder={t("加标签")} value={tag} onChange={(e) => setTag(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") { void addTag(db, [conv.key], tag).then(load); setTag(""); } }} />
      </div>
      <textarea style={{ width: "100%", marginTop: 6 }} rows={4} placeholder={t("备注")} key={conv.key} defaultValue={conv.note} onBlur={(e) => { if (e.target.value !== conv.note) void save({ note: e.target.value }); }} />
    </>}
    <h4>{t("摘录")}（{excerpts.length}）</h4>
    <p className="mut">{t("在网页上选中文字，点出现的「存为摘录」按钮即可添加。")}</p>
    {excerpts.map((x) => <div key={x.id} className="msg">{x.text}
      <div><button onClick={() => { if (db) void deleteExcerpt(db, x.id).then(load); }}>{t("删除")}</button></div></div>)}
    <button onClick={() => void chrome.tabs.create({ url: chrome.runtime.getURL("manage.html") })}>{t("在管理页打开")}</button>
  </div>;
}
