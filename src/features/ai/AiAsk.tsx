import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { Modal } from "../../ui";
import { aiError } from "./AiSettings";

/** One of the questions a page can put to the AI source; see `ai_features.rs`. */
export type AiKind = "install_failure" | "checkup" | "toolchain" | "disk_batch" | "gateway_error" | "proxy" | "lan_address";

export const askAi = (kind: AiKind, payload: unknown) => invoke<string>("ai_ask", { kind, payload });

/** The app shell listens for this and opens the page named in `detail`. */
export const GOTO_EVENT = "stacker:goto";
export const openAiSettings = () => window.dispatchEvent(new CustomEvent(GOTO_EVENT, { detail: "settings" }));

/** Failures that Preferences → AI fixes: no source chosen, a source that is incomplete, an agent that cannot answer. */
export function needsAiSetup(error: unknown): boolean {
  return /^E_(AI_(NONE|MODEL|FIELDS|KEY|KIND|PROTOCOL|EFFORT)|RUNNER_(AUTH|NO_PLAN|MISSING))$/.test(String(error));
}

/** What every AI dialog shows: waiting, the answer, or the failure with the way to fix it. */
export function useAiAnswer(run: () => Promise<string>, deps: unknown[]) {
  const [answer, setAnswer] = useState<string | null>(null);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    let alive = true;
    run()
      .then((text) => { if (alive) setAnswer(text.trim()); })
      .catch((e) => { if (alive) setError(e ?? "E_AI_REPLY"); });
    return () => { alive = false; };
    // The caller names what a new question depends on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return { answer, error };
}

export function AiAnswer({ answer, error, waiting }: { answer: string | null; error: unknown; waiting: string }) {
  const { tr } = useI18n();
  if (error !== null) {
    return <div className="ai-explain bad">
      <i className="ti ti-alert-circle" /> {tr(aiError(error))}
      {needsAiSetup(error) && <div className="ai-explain-fix">{tr("Stacker 的 AI 功能都用「偏好设置 → AI 能力」里选的来源。")}</div>}
    </div>;
  }
  if (answer !== null) return <div className="ai-explain">{answer}</div>;
  return <div className="ai-explain wait"><i className="ti ti-loader spin" /> {tr(waiting)}</div>;
}

/** The dialog's buttons: copy the answer, go and set the AI up when that is what is missing, close. */
export function AiFooter({ answer, error, onClose, onSetup }: {
  answer: string | null; error: unknown; onClose: () => void;
  /** Closes whatever else sits above the page before Preferences opens; `onClose` when omitted. */
  onSetup?: () => void;
}) {
  const { tr } = useI18n();
  const setup = needsAiSetup(error);
  return <>
    {answer && <button className="gh sm" onClick={() => void navigator.clipboard.writeText(answer)}><i className="ti ti-copy" /> {tr("复制")}</button>}
    <button className={setup ? "gh sm" : "pr sm"} onClick={onClose}>{tr("关闭")}</button>
    {setup && <button className="pr sm" onClick={() => { (onSetup ?? onClose)(); openAiSettings(); }}><i className="ti ti-settings" /> {tr("去配置 AI")}</button>}
  </>;
}

/**
 * A question to the AI source and its answer. `run` does the asking, so a page can gather
 * what it needs first (a log, a probe) and still show one dialog for the whole thing.
 */
export function AiAskModal({ title, sub, note, run, onClose, onSetup, waiting = "AI 正在看…" }: {
  title: string;
  sub?: ReactNode;
  /** What was sent, said plainly under the answer. */
  note: string;
  run: () => Promise<string>;
  onClose: () => void;
  onSetup?: () => void;
  waiting?: string;
}) {
  // One question per dialog: asking again means opening it again.
  const { answer, error } = useAiAnswer(run, []);
  return <Modal wide title={title} icon="ti-sparkles" sub={sub} onClose={onClose}
    footer={<AiFooter answer={answer} error={error} onClose={onClose} onSetup={onSetup} />}>
    <AiAnswer answer={answer} error={error} waiting={waiting} />
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

/** For an AI call whose result is not a dialog (a filter, a fill-in): the dialog shown only when the AI is not set up. */
export function AiSetupPrompt({ error, onClose }: { error: unknown; onClose: () => void }) {
  const { tr } = useI18n();
  return <Modal title={tr("需要先配置 AI")} icon="ti-sparkles" onClose={onClose}
    footer={<AiFooter answer={null} error={error} onClose={onClose} />}>
    <AiAnswer answer={null} error={error} waiting="" />
  </Modal>;
}
