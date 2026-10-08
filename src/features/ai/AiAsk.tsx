import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { Modal } from "../../ui";
import { aiError } from "./AiSettings";

/** One of the questions a page can put to the AI source; see `ai_features.rs`. */
export type AiKind = "install_failure" | "checkup" | "toolchain" | "disk_directory" | "gateway_error" | "proxy" | "lan_address";

export const askAi = (kind: AiKind, payload: unknown) => invoke<string>("ai_ask", { kind, payload });

/** The app shell listens for this and opens the page named in `detail`. */
export const GOTO_EVENT = "stacker:goto";
export const openAiSettings = () => window.dispatchEvent(new CustomEvent(GOTO_EVENT, { detail: "settings" }));

/** Failures that Preferences → AI fixes: no source chosen, a source that is incomplete, an agent that cannot answer. */
export function needsAiSetup(error: unknown): boolean {
  return /^E_(AI_(NONE|MODEL|FIELDS|KEY|KIND|PROTOCOL|EFFORT)|RUNNER_(AUTH|NO_PLAN|MISSING))$/.test(String(error));
}

const SAVED_KEY = "stacker.aiAnswers.v1";
const SAVED_LIMIT = 40;
type Saved = { answer: string; at: number };

function savedAll(): Record<string, Saved> {
  try { return JSON.parse(localStorage.getItem(SAVED_KEY) ?? "{}") as Record<string, Saved>; } catch { return {}; }
}

/** The answer last given for this question, kept on this computer. */
export function savedAnswer(key: string): Saved | null {
  const item = savedAll()[key];
  return item && typeof item.answer === "string" ? item : null;
}

/** Keeps an answer, dropping the oldest once there are more than a few dozen. */
export function saveAnswer(key: string, answer: string, at = Date.now()): void {
  const all = { ...savedAll(), [key]: { answer, at } };
  const kept = Object.entries(all).sort((a, b) => b[1].at - a[1].at).slice(0, SAVED_LIMIT);
  try { localStorage.setItem(SAVED_KEY, JSON.stringify(Object.fromEntries(kept))); } catch { /* the answer is still on screen */ }
}

/** What every AI dialog shows: waiting, the answer, or the failure with the way to fix it. */
export function useAiAnswer(run: () => Promise<string>, deps: unknown[]) {
  const [answer, setAnswer] = useState<string | null>(null);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    let alive = true;
    setAnswer(null);
    setError(null);
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
export function AiAskModal({ title, sub, note, run, onClose, onSetup, waiting = "AI 正在看…", saveAs }: {
  title: string;
  sub?: ReactNode;
  /** What was sent, said plainly under the answer. */
  note: string;
  run: () => Promise<string>;
  onClose: () => void;
  onSetup?: () => void;
  waiting?: string;
  /** Keep the answer under this name: opening the dialog again shows it without asking again. */
  saveAs?: string;
}) {
  const { tr } = useI18n();
  const [kept] = useState(() => (saveAs ? savedAnswer(saveAs) : null));
  // Asking again is a new question: the counter starts one.
  const [round, setRound] = useState(kept ? 0 : 1);
  const { answer, error } = useAiAnswer(round === 0 ? async () => kept!.answer : async () => {
    const text = await run();
    if (saveAs) saveAnswer(saveAs, text.trim());
    return text;
  }, [round]);
  const shownAt = round === 0 && kept ? new Date(kept.at).toLocaleString() : null;
  return <Modal wide title={title} icon="ti-sparkles" sub={sub} onClose={onClose}
    footer={<>
      {saveAs && (answer !== null || error !== null) && <button className="gh sm" onClick={() => setRound((n) => n + 1)}><i className="ti ti-refresh" /> {tr("重新诊断")}</button>}
      <AiFooter answer={answer} error={error} onClose={onClose} onSetup={onSetup} />
    </>}>
    {shownAt && <p className="s dim" style={{ margin: 0 }}>{tr("上次的诊断结果（{at}）。要按现在的情况再问一次，点「重新诊断」。").replace("{at}", shownAt)}</p>}
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
