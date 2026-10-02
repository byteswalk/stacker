import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { Modal } from "../../ui";
import { AiAnswer, AiFooter, useAiAnswer } from "./AiAsk";

/** What a folder is and whether deleting it hurts, asked of whichever AI Preferences names. */
export function AiExplain({ path, bytes, kind, onClose }: {
  path: string; bytes: number; kind: string; onClose: () => void;
}) {
  const { tr } = useI18n();
  const { answer, error } = useAiAnswer(() => invoke<string>("ai_explain_path", { path, bytes, kind }), [path, bytes, kind]);

  return <Modal title={tr("问问 AI")} icon="ti-sparkles" sub={path} onClose={onClose}
    footer={<AiFooter answer={answer} error={error} onClose={onClose} />}>
    <AiAnswer answer={answer} error={error} waiting="正在请 AI 看这个目录…" />
    <p className="s dim ai-explain-note">{tr("只把路径、大小和类型发给 AI，不读取目录里的文件。回答仅供参考，删前以清理确认为准。")}</p>
  </Modal>;
}
