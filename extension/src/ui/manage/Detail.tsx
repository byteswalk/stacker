import { useEffect, useState } from "react";
import { t } from "../../i18n";
import { addTag, getBody, listExcerpts, updateLocal, type Conversation, type Db, type Excerpt, type Folder, type StoredBody } from "../../lib/db";
import { conversationUrl } from "../../sites/registry";

export function Detail({ db, conv, folders, onRead, onChanged, reading }: {
  db: Db; conv: Conversation; folders: Folder[]; reading: boolean;
  onRead: (c: Conversation) => void; onChanged: () => void;
}) {
  const [body, setBody] = useState<StoredBody | undefined>();
  const [excerpts, setExcerpts] = useState<Excerpt[]>([]);
  const [note, setNote] = useState(conv.note);
  const [tag, setTag] = useState("");
  useEffect(() => {
    setNote(conv.note);
    void getBody(db, conv.key).then(setBody);
    void listExcerpts(db, conv.site, conv.id).then(setExcerpts);
  }, [db, conv]);
  const url = conversationUrl(conv.site, conv.id);
  const save = async (patch: Parameters<typeof updateLocal>[2]) => { await updateLocal(db, [conv.key], patch); onChanged(); };

  return <div>
    <h3>{conv.title || t("（无标题）")}</h3>
    <p><a href={url} target="_blank" rel="noreferrer">{t("打开原对话")}</a></p>
    <div className="row">
      <label><input type="checkbox" checked={conv.favorite} onChange={(e) => void save({ favorite: e.target.checked })} /> {t("收藏")}</label>
      <select value={conv.folderId ?? ""} onChange={(e) => void save({ folderId: e.target.value || null })}>
        <option value="">{t("未归入文件夹")}</option>
        {folders.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
      </select>
    </div>
    <div className="row" style={{ marginTop: 6 }}>
      {conv.tags.map((x) => <span key={x} className="chip">{x} <button aria-label={t("移除标签")} onClick={() => void save({ tags: conv.tags.filter((y) => y !== x) })}>×</button></span>)}
      <input placeholder={t("加标签")} value={tag} onChange={(e) => setTag(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") { void addTag(db, [conv.key], tag).then(onChanged); setTag(""); } }} />
    </div>
    <textarea style={{ width: "100%", marginTop: 6 }} rows={3} placeholder={t("备注")} value={note} onChange={(e) => setNote(e.target.value)} onBlur={() => { if (note !== conv.note) void save({ note }); }} />
    <div className="row"><button disabled={reading || conv.removedAt !== null} onClick={() => onRead(conv)}>{t(body ? "重新读取正文" : "读取正文")}</button>
      {body && <span className="mut">{t("读取于")} {new Date(conv.bodyFetchedAt ?? 0).toLocaleString()}</span>}</div>
    {excerpts.length > 0 && <><h4>{t("摘录")}</h4>{excerpts.map((x) => <div key={x.id} className="msg">{x.text}{x.note && <div className="mut">{x.note}</div>}</div>)}</>}
    {body?.messages.map((m, i) => <div key={i} className="msg"><b>{m.role === "user" ? t("用户") : m.role === "assistant" ? t("助手") : m.role}</b>{"\n"}{m.text}</div>)}
  </div>;
}
