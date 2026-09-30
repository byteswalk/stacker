import { useEffect, useState } from "react";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { Modal } from "../../ui";
import { aiError } from "./AiSettings";

/** What a folder is and whether deleting it hurts, asked of whichever AI Preferences names. */
export function AiExplain({ path, bytes, kind, onClose }: {
  path: string; bytes: number; kind: string; onClose: () => void;
}) {
  const { tr } = useI18n();
  const [answer, setAnswer] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    invoke<string>("ai_explain_path", { path, bytes, kind })
      .then((text) => { if (alive) setAnswer(text.trim()); })
      .catch((e) => { if (alive) setError(tr(aiError(e))); });
    return () => { alive = false; };
  }, [path, bytes, kind, tr]);

  return <Modal title={tr("问问 AI")} icon="ti-sparkles" sub={path} onClose={onClose}
    footer={<button className="pr sm" onClick={onClose}>{tr("关闭")}</button>}>
    {!answer && !error && <div className="ai-explain wait"><i className="ti ti-loader spin" /> {tr("正在请 AI 看这个目录…")}</div>}
    {answer && <div className="ai-explain">{answer}</div>}
    {error && <div className="ai-explain bad"><i className="ti ti-alert-circle" /> {error}</div>}
    <p className="s dim ai-explain-note">{tr("只把路径、大小和类型发给 AI，不读取目录里的文件。回答仅供参考，删前以清理确认为准。")}</p>
  </Modal>;
}
