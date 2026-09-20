import { useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ConfirmModal, Modal, useBusy, useBusyRead } from "../../ui";
import { cancelWebSummary, readWebChat, summarizeWebChat } from "./api";
import { runnerText } from "./SummaryDialog";
import { errorMessage, webSiteLabel, type WebChat, type WebChatDetail as Detail } from "./types";

const ROLE: Record<string, string> = { user: "用户", assistant: "助手", tool: "工具", system: "系统" };

/** One web chat: its stored body, local notes and summary. */
export function WebChatDetail({ chat, onClose, onDistill }: { chat: WebChat; onClose: (changed: boolean) => void; onDistill?: (key: string) => void }) {
  const { tr: t, locale } = useI18n();
  const read = useBusyRead();
  const busy = useBusy();
  const [detail, setDetail] = useState<Detail | null>(null);
  const [asking, setAsking] = useState(false);
  const [changed, setChanged] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    read("正在读取网页对话正文", () => readWebChat(chat.key))
      .then(setDetail)
      .catch((e) => setError(errorMessage(e)));
  }, [chat.key, read]);

  async function summarize() {
    setAsking(false); setError("");
    try {
      const updated = await busy({
        title: "正在生成摘要",
        message: "摘要由本机智能体生成，长对话需要几分钟。",
        cancel: { label: "取消", onCancel: () => void cancelWebSummary() },
      }, () => summarizeWebChat(chat.key, null, locale));
      setDetail((d) => (d ? { ...d, chat: updated } : d));
      setChanged(true);
    } catch (e) { setError(errorMessage(e)); }
  }

  const c = detail?.chat ?? chat;
  const hasBody = !!detail && detail.messages.length > 0;
  return <Modal wide title={c.title || t("（无标题）")} onClose={() => onClose(changed)}>
    <div className="session-detail-meta">
      <span>{webSiteLabel(c.site)} · {c.accountName}</span>
      <span>{new Date(c.updatedAt).toLocaleString(locale)}</span>
      {c.folder && <span>{c.folder}</span>}
      {c.tags.length > 0 && <span>{c.tags.join("、")}</span>}
      {c.removedAt !== null && <span>{t("网站上已删除")}</span>}
    </div>
    {c.note && <p className="session-note">{c.note}</p>}
    <div className="session-summary">
      <div className="session-summary-head">
        <b>{t("摘要")}{c.summaryStale && <em>{t("已过期")}</em>}</b>
        {c.summary && c.summaryBy && <small>{c.summaryBy} · {new Date(c.summaryAt).toLocaleString(locale)}</small>}
        <button className="gh sm" disabled={!hasBody} onClick={() => setAsking(true)}><i className="ti ti-sparkles" />{t(c.summary ? "重新生成" : "生成摘要")}</button>
        {onDistill && <button className="gh sm" disabled={!hasBody} onClick={() => { onClose(changed); onDistill(c.key); }}><i className="ti ti-bulb" />{t("提炼…")}</button>}
      </div>
      {c.summary ? <pre translate="no">{c.summary}</pre> : <p className="session-note">{t("还没有摘要。")}</p>}
    </div>
    {error && <p role="alert" className="session-error">{t(error)}</p>}
    {detail && !hasBody && <p className="session-note">{t("Stacker 还没有这条对话的正文：在插件里打开它并点「读取正文」，同步后即可在这里查看和生成摘要。")}</p>}
    {hasBody && c.bodyStale && <p className="session-note">{t("网站上的对话在读取正文后又有更新，这里显示的是上次读取的内容。")}</p>}
    <div className="session-transcript" translate="no">
      {detail?.messages.map((m, i) => <article key={i} className={m.role}><header><b>{t(ROLE[m.role] ?? m.role)}</b></header><pre>{m.text}</pre></article>)}
    </div>
    {asking && detail && <ConfirmModal title={t("生成摘要")} icon="ti-sparkles"
      message={`${t("将把这条对话的正文")}（${detail.chars} ${t("字")}）${t("发送给")} ${runnerText(detail.runner, t)} ${t("生成摘要，消耗该账号的额度。执行者、模型与推理强度沿用「设置 → 摘要」。")}`}
      confirmLabel={t("生成摘要")} onConfirm={() => void summarize()} onClose={() => setAsking(false)} />}
  </Modal>;
}
