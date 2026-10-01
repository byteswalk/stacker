import { useEffect, useState } from "react";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { Modal } from "../../ui";
import { aiError } from "./AiSettings";

/** One of the questions a page can put to the AI source; see `ai_features.rs`. */
export type AiKind = "install_failure" | "checkup" | "toolchain" | "disk_batch" | "gateway_error" | "proxy" | "lan_address";

export const askAi = (kind: AiKind, payload: unknown) => invoke<string>("ai_ask", { kind, payload });

/**
 * A question to the AI source and its answer. `run` does the asking, so a page can gather
 * what it needs first (a log, a probe) and still show one dialog for the whole thing.
 */
export function AiAskModal({ title, sub, note, run, onClose }: {
  title: string;
  sub?: string;
  /** What was sent, said plainly under the answer. */
  note: string;
  run: () => Promise<string>;
  onClose: () => void;
}) {
  const { tr } = useI18n();
  const [answer, setAnswer] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    run()
      .then((text) => { if (alive) setAnswer(text.trim()); })
      .catch((e) => { if (alive) setError(tr(aiError(e))); });
    return () => { alive = false; };
    // One question per dialog: asking again means opening it again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return <Modal wide title={title} icon="ti-sparkles" sub={sub} onClose={onClose}
    footer={<>
      {answer && <button className="gh sm" onClick={() => void navigator.clipboard.writeText(answer)}><i className="ti ti-copy" /> {tr("复制")}</button>}
      <button className="pr sm" onClick={onClose}>{tr("关闭")}</button>
    </>}>
    {!answer && !error && <div className="ai-explain wait"><i className="ti ti-loader spin" /> {tr("AI 正在看…")}</div>}
    {answer && <div className="ai-explain">{answer}</div>}
    {error && <div className="ai-explain bad"><i className="ti ti-alert-circle" /> {error}</div>}
    <p className="s dim ai-explain-note">{note}</p>
  </Modal>;
}

/** The button every page uses to open one, so they all look the same. */
export function AiButton({ label, title, disabled, onClick, small = true }: {
  label: string; title?: string; disabled?: boolean; onClick: () => void; small?: boolean;
}) {
  return <button className={"gh ai-btn" + (small ? " sm" : "")} title={title} disabled={disabled} onClick={onClick}>
    <i className="ti ti-sparkles" /> {label}
  </button>;
}
