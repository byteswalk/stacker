import { createContext, useContext, useState, useCallback, useEffect, useId, useRef, type ReactNode } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useModalFocus } from "./modalFocus";
import { reportFrontendWarning } from "./invoke";
import { useI18n } from "./i18n";

/* ───────────── Toast ───────────── */
type ToastKind = "ok" | "err" | "info";
type ToastItem = { id: number; msg: string; full: string; kind: ToastKind };
const ToastCtx = createContext<{
  push: (msg: string, kind?: ToastKind) => void;
  dismiss: (id: number) => void;
  items: ToastItem[];
}>({
  push: () => {}, dismiss: () => {}, items: [],
});
let _tid = 0;

function normalizeToastMessage(value: string) {
  const full = String(value ?? "")
    .replace(/^Error:\s*/i, "")
    .replace(/\r\n/g, "\n")
    .trim() || "操作未完成";
  if (full.length <= 720) return { msg: full, full };
  const logLine = full.split("\n").find((line) => /(?:诊断|安装)日志[：:]/.test(line));
  const suffix = logLine ? `\n${logLine.trim()}` : "";
  return { msg: `${full.slice(0, 620).trimEnd()}…${suffix}`, full };
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const currentItems = useRef<ToastItem[]>([]);
  const timers = useRef(new Map<number, number>());
  useEffect(() => {
    const activeTimers = timers.current;
    return () => { activeTimers.forEach(clearTimeout); activeTimers.clear(); };
  }, []);
  const dismiss = useCallback((id: number) => {
    clearTimeout(timers.current.get(id));
    timers.current.delete(id);
    currentItems.current = currentItems.current.filter((item) => item.id !== id);
    setItems(currentItems.current);
  }, []);
  const push = useCallback((msg: string, kind: ToastKind = "ok") => {
    const normalized = normalizeToastMessage(msg);
    const existing = currentItems.current.find((item) => item.kind === kind && item.full === normalized.full);
    const id = existing?.id ?? ++_tid;
    clearTimeout(timers.current.get(id));
    const next = [...currentItems.current.filter((item) => item.id !== id), { id, ...normalized, kind }];
    while (next.length > 3) {
      const removed = next.shift()!;
      clearTimeout(timers.current.get(removed.id));
      timers.current.delete(removed.id);
    }
    currentItems.current = next;
    setItems(next);
    const duration = kind === "err" ? 9000 : kind === "info" ? 5500 : 3500;
    timers.current.set(id, window.setTimeout(() => dismiss(id), duration));
  }, [dismiss]);
  return <ToastCtx.Provider value={{ push, dismiss, items }}>{children}</ToastCtx.Provider>;
}
export function useToast() { return useContext(ToastCtx).push; }

export function operationWasCancelled(error: unknown) {
  return /(?:已取消|取消操作|cancelled|canceled|operation aborted)/i.test(String(error));
}

const TOAST_ICON: Record<ToastKind, string> = { ok: "ti-circle-check", err: "ti-alert-circle", info: "ti-info-circle" };
/** Render inside `.a` so the scoped `.a .toast` styles + CSS vars apply. */
export function ToastHost() {
  const { items, dismiss } = useContext(ToastCtx);
  return (
    <div style={{ position: "fixed", left: "50%", bottom: 18, transform: "translateX(-50%)", zIndex: 100, display: "flex", flexDirection: "column", gap: 8, alignItems: "center", pointerEvents: "none" }}>
      {items.map((t) => (
        <div key={t.id} role={t.kind === "err" ? "alert" : "status"} className={"toast " + t.kind} title={t.full !== t.msg ? t.full : undefined}
          style={{ position: "static", transform: "none", pointerEvents: "auto" }}>
          <i className={"ti " + TOAST_ICON[t.kind]} />
          <span className="toast-text">{t.msg}</span>
          <button className="close" aria-label="关闭提示" title="关闭" onClick={() => dismiss(t.id)}><i className="ti ti-x" /></button>
        </div>
      ))}
    </div>
  );
}

/* ───────────── Busy（全局进度模态：长操作期间挡住切页，可取消/转后台） ───────────── */
export type BusyOpts = {
  title: string;
  message?: string;
  progressEvent?: string;            // 订阅的进度事件名（下载=install-progress，扫描=env-scan-progress）
  doneToken?: string;                // 视为完成的 payload（默认 __done__）
  cancel?: { label: string; onCancel: () => void }; // 真取消按钮（仅可取消的操作，如扫描）
  /** Show the dialog only if the task is still running after this long, so quick reads never flash. */
  delayMs?: number;
  /** Reads may overlap each other and an action; actions (the default) stay one at a time. */
  shared?: boolean;
};
type BusyEntry = BusyOpts & { id: number; visible: boolean; progress?: string; cancelRequested?: boolean };
type BusyState = BusyEntry | null;
const BusyCtx = createContext<{
  state: BusyState;
  run: <T>(opts: BusyOpts, task: () => Promise<T>) => Promise<T>;
  hide: () => void;
  requestCancel: () => void;
}>({ state: null, run: async (_o, t) => t(), hide: () => {}, requestCancel: () => {} });

/** The delay used for reads that are usually quick but can take seconds. */
export const BUSY_READ_DELAY = 400;

export function BusyProvider({ children }: { children: ReactNode }) {
  const [entries, setEntries] = useState<BusyEntry[]>([]);
  const activeRef = useRef(false);
  const seq = useRef(0);
  const patch = useCallback((id: number, change: (entry: BusyEntry) => BusyEntry) => {
    setEntries((list) => list.map((entry) => entry.id === id ? change(entry) : entry));
  }, []);
  const state = [...entries].reverse().find((entry) => entry.visible) ?? null;
  const shownId = state?.id;
  const hide = useCallback(() => setEntries([]), []);
  const requestCancel = useCallback(() => {
    if (shownId === undefined) return;
    patch(shownId, (entry) => ({ ...entry, cancelRequested: true, progress: "正在取消，请稍候…" }));
  }, [patch, shownId]);
  const run = useCallback(async <T,>(opts: BusyOpts, task: () => Promise<T>): Promise<T> => {
    const exclusive = !opts.shared;
    if (exclusive && activeRef.current) throw new Error("已有操作正在执行，请等待当前操作完成。");
    if (exclusive) activeRef.current = true;
    const id = ++seq.current;
    setEntries((list) => [...list, { ...opts, id, visible: !opts.delayMs }]);
    const timer = opts.delayMs ? window.setTimeout(() => patch(id, (entry) => ({ ...entry, visible: true })), opts.delayMs) : undefined;
    let un: UnlistenFn | undefined;
    try {
      if (opts.progressEvent) {
        const done = opts.doneToken ?? "__done__";
        // 节流：扫描进度每秒可达数百条，逐条 setState 会卡死模态/取消按钮，限到 ~8 次/秒（"完成"立即）
        let last = 0;
        un = await listen<string>(opts.progressEvent, (e) => {
          const isDone = e.payload === done;
          const now = Date.now();
          if (!isDone && now - last < 120) return;
          last = now;
          patch(id, (entry) => entry.cancelRequested ? entry : { ...entry, progress: isDone ? "完成" : e.payload });
        });
      }
      return await task();
    }
    finally {
      if (timer !== undefined) window.clearTimeout(timer);
      if (exclusive) activeRef.current = false;
      setEntries((list) => list.filter((entry) => entry.id !== id));
      try {
        void Promise.resolve(un?.()).catch((cause) => reportFrontendWarning("Progress listener cleanup failed", cause));
      } catch (cause) {
        reportFrontendWarning("Progress listener cleanup failed", cause);
      }
    }
  }, [patch]);
  return <BusyCtx.Provider value={{ state, run, hide, requestCancel }}>{children}</BusyCtx.Provider>;
}
/** 返回 run：await busy({title,...}, () => invoke(...))，期间弹模态挡操作。 */
export function useBusy() { return useContext(BusyCtx).run; }

/**
 * For reads: `await read(title, () => invoke(...))` shows the progress dialog only when the read
 * takes longer than a moment, and never refuses to run because something else is busy.
 */
export function useBusyRead() {
  const run = useBusy();
  return useCallback(<T,>(title: string, task: () => Promise<T>, message?: string) =>
    run({ title, message, shared: true, delayMs: BUSY_READ_DELAY }, task), [run]);
}

export function BusyHost() {
  const { state, requestCancel } = useContext(BusyCtx);
  if (!state) return null;
  // A read never covers the window: a small note says it is going on, and the user can go on
  // to another page; the page shows the result when it comes.
  if (state.shared && !state.cancel && !state.progressEvent) {
    return <BusyNote title={state.title} />;
  }
  return <BusyDialog state={state} requestCancel={requestCancel} />;
}

function BusyNote({ title }: { title: string }) {
  const { tr } = useI18n();
  return <div className="busy-note" role="status" aria-live="polite"><i className="ti ti-loader spin" /> {tr(title)}…</div>;
}

function BusyDialog({ state, requestCancel }: { state: NonNullable<BusyState>; requestCancel: () => void }) {
  const { tr } = useI18n();
  const modalRef = useModalFocus(undefined, 200);
  const titleId = useId();
  return (
    <div className="modalmask busy-mask" style={{ zIndex: 200 }}>
      <div ref={modalRef} tabIndex={-1} role="dialog" aria-modal="true" aria-labelledby={titleId} aria-busy="true" className="modal" style={{ maxWidth: 470 }}>
        <div className="modalhd"><span id={titleId}><i className="ti ti-loader spin" /> {tr(state.title)}</span></div>
        <div className="modalbody">
          {state.message && <div style={{ fontSize: 13, color: "var(--tx)", lineHeight: 1.7 }}>{tr(state.message)}</div>}
          {(state.progress || state.progressEvent) && (
            <div className="instbar trace-card" style={{ margin: "10px 0 0", overflow: "hidden", flexDirection: "column", alignItems: "stretch", gap: 8 }}>
              <span className="border-runner" aria-hidden="true" />
              <div style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0 }}>
              <i className="ti ti-loader spin" style={{ color: "var(--acc)" }} />
              <span className="ptxt" style={{ flex: 1, minWidth: 0, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }} title={tr(state.progress ?? "正在处理")}>{tr(state.progress ?? "正在处理…")}</span>
              </div>
            </div>
          )}
          <div style={{ fontSize: 11.5, color: "var(--mut)", marginTop: 10, lineHeight: 1.6 }}>
            {tr("请保持 Stacker 运行。操作完成后，此窗口会自动关闭。")}</div>
        </div>
        {state.cancel && (
          <div className="modalft">
            <button className="gh sm" disabled={state.cancelRequested} onClick={() => {
              requestCancel();
              state.cancel!.onCancel();
            }}>{tr(state.cancelRequested ? "正在取消…" : state.cancel.label)}</button>
          </div>
        )}
      </div>
    </div>
  );
}

/* ───────────── 加载占位（统一的"检测中"动画） ───────────── */
export function Loading({ text }: { text: string }) {
  return (
    <div className="stub load-card trace-card">
      <span className="border-runner" />
      <div className="si"><i className="ti ti-loader spin" /></div>
      <div>
        <h2>正在读取环境状态</h2>
        <p>{text}</p>
      </div>
    </div>
  );
}

export function ErrorState({ title, description, onRetry }: {
  title: string;
  description: string;
  onRetry?: () => void | Promise<void>;
}) {
  const [retrying, setRetrying] = useState(false);
  async function retry() {
    if (!onRetry || retrying) return;
    setRetrying(true);
    try { await onRetry(); } finally { setRetrying(false); }
  }
  return (
    <div className="stub">
      <div className="si"><i className="ti ti-plug-x" /></div>
      <h2>{title}</h2>
      <p>{description}</p>
      {onRetry && <button className="pr sm" disabled={retrying} onClick={retry}>
        <i className={"ti " + (retrying ? "ti-loader spin" : "ti-refresh")} /> {retrying ? "重试中…" : "重试"}
      </button>}
    </div>
  );
}

/* ───────────── Modal ───────────── */
export function Modal({ title, icon, sub, wide, children, footer, onClose }: {
  title: ReactNode; icon?: string; sub?: ReactNode; wide?: boolean;
  children?: ReactNode; footer?: ReactNode; onClose?: () => void;
}) {
  const modalRef = useModalFocus(onClose);
  const titleId = useId();
  return (
    <div className="modalmask">
      <div ref={modalRef} tabIndex={-1} role="dialog" aria-modal="true" aria-labelledby={titleId} className={"modal" + (wide ? " wide" : "")}>
        <div className="modalhd">
          <span id={titleId}>{icon && <i className={"ti " + icon} />} {title}</span>
          {onClose && <button className="ic" aria-label="关闭" title="关闭" onClick={onClose}><i className="ti ti-x" /></button>}
        </div>
        {sub != null && <div className="modalsub">{sub}</div>}
        <div className="modalbody">{children}</div>
        {footer != null && <div className="modalft">{footer}</div>}
      </div>
    </div>
  );
}

/* ───────────── Confirm（破坏性二次确认） ───────────── */
export function ConfirmModal({ title, icon, message, confirmLabel = "确认", danger, busy, onConfirm, onClose }: {
  title: ReactNode; icon?: string; message: ReactNode; confirmLabel?: string;
  danger?: boolean; busy?: boolean; onConfirm: () => void; onClose: () => void;
}) {
  return (
    <Modal title={title} icon={icon ?? (danger ? "ti-alert-triangle" : "ti-help-circle")} onClose={busy ? undefined : onClose}
      footer={<>
        <button className="gh sm" onClick={onClose} disabled={busy}>取消</button>
        <button className={"pr sm" + (danger ? " danger-solid" : "")} style={danger ? { background: "#d6463d" } : undefined}
          onClick={onConfirm} disabled={busy}>{confirmLabel}</button>
      </>}>
      <div style={{ fontSize: 13, lineHeight: 1.7, color: "var(--tx)" }}>{message}</div>
    </Modal>
  );
}
