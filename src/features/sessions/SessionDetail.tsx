import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { Modal, useBusyRead } from "../../ui";
import { formatSpaceBytes as bytes } from "../space-analysis/components/SpaceOverview";
import { openSession, readSession } from "./api";
import { AGENT_LABEL, CLIENT_LABEL, STATUS_LABEL } from "./sessionsView";
import { errorMessage, type Session, type SessionDetail as Detail } from "./types";

const DETAIL_PAGE = 200;
const ROLE: Record<string, string> = { user: "用户", assistant: "助手", tool: "工具" };

export function SessionDetail({ session, onClose, onSummarize }: { session: Session; onClose: () => void; onSummarize: () => void }) {
  const { tr: t, locale } = useI18n();
  const [detail, setDetail] = useState<Detail | null>(null);
  const [offset, setOffset] = useState(0);
  const [error, setError] = useState("");
  const request = useRef(0);
  const read = useBusyRead();

  useEffect(() => {
    const current = ++request.current;
    read("正在读取会话记录", () => readSession(session.id, offset))
      .then((d) => { if (current === request.current) setDetail(d); })
      .catch((e) => { if (current === request.current) setError(errorMessage(e)); });
  }, [session.id, offset, read]);

  const open = (target: "folder" | "project" | "native") => void openSession(session.id, target).catch((e) => setError(errorMessage(e)));
  return <Modal wide title={session.title} onClose={onClose} footer={<>
    <button className="gh sm" onClick={() => open("folder")}><i className="ti ti-folder" />{t("打开记录所在文件夹")}</button>
    {session.project.exists && <button className="gh sm" onClick={() => open("project")}><i className="ti ti-code" />{t("打开项目")}</button>}
    {session.agent === "codex" && <button className="pr sm" onClick={() => open("native")}>{t("在 Codex 中打开")}</button>}
  </>}>
    <div className="session-detail-meta">
      <span>{AGENT_LABEL[session.agent]} · {t(CLIENT_LABEL[session.client])}</span>
      <span>{t(STATUS_LABEL[session.status])}</span>
      <span>{bytes(session.bytes)}</span>
      <span>{new Date(session.updatedAt * 1000).toLocaleString(locale)}</span>
      <code title={session.project.path}>{session.project.path}</code>
    </div>
    <div className="session-summary">
      <div className="session-summary-head">
        <b>{t("摘要")}{session.summaryStale && <em>{t("已过期")}</em>}</b>
        {session.summary && session.summaryBy && <small>{session.summaryBy} · {new Date(session.summaryAt * 1000).toLocaleString(locale)}</small>}
        <button className="gh sm" onClick={onSummarize}><i className="ti ti-sparkles" />{t(session.summary ? "重新生成" : "生成摘要")}</button>
      </div>
      {session.summary ? <pre translate="no">{session.summary}</pre> : <p className="session-note">{t("还没有摘要。用本机智能体生成后可在列表中搜索，并写入精简导出。")}</p>}
    </div>
    {error && <p role="alert" className="session-error">{t(error)}</p>}
    {detail && !detail.complete && <p className="session-error">{t("部分内容无法读取，仅显示可读部分。")}</p>}
    <div className="session-transcript" translate="no">
      {detail?.messages.map((m) => <article key={m.line} className={m.role}><header><b>{t(ROLE[m.role] ?? m.role)}</b><code>L{m.line}</code></header><pre>{m.text}</pre></article>)}
    </div>
    {detail && detail.total > DETAIL_PAGE && <div className="session-pagination">
      <span />
      <button className="gh sm" disabled={!offset} onClick={() => setOffset(Math.max(0, offset - DETAIL_PAGE))}>{t("上一页")}</button>
      <span>{offset + 1}–{Math.min(detail.total, offset + DETAIL_PAGE)} / {detail.total}</span>
      <button className="gh sm" disabled={offset + DETAIL_PAGE >= detail.total} onClick={() => setOffset(offset + DETAIL_PAGE)}>{t("下一页")}</button>
    </div>}
  </Modal>;
}
