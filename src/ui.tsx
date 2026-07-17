import { createContext, useContext, useState, useCallback, useEffect, useId, useRef, type ReactNode } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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
  const dismiss = useCallback((id: number) => {
    setItems((items) => items.filter((item) => item.id !== id));
  }, []);
  const push = useCallback((msg: string, kind: ToastKind = "ok") => {
    const id = ++_tid;
    const normalized = normalizeToastMessage(msg);
    setItems((items) => [...items, { id, ...normalized, kind }]);
    const duration = kind === "err" ? 9000 : kind === "info" ? 5500 : 3500;
    window.setTimeout(() => dismiss(id), duration);
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
        <div key={t.id} className={"toast " + t.kind} title={t.full !== t.msg ? t.full : undefined}
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
};
type BusyState = (BusyOpts & { progress?: string; cancelRequested?: boolean }) | null;
const BusyCtx = createContext<{
  state: BusyState;
  run: <T>(opts: BusyOpts, task: () => Promise<T>) => Promise<T>;
  hide: () => void;
  requestCancel: () => void;
}>({ state: null, run: async (_o, t) => t(), hide: () => {}, requestCancel: () => {} });

export function BusyProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<BusyState>(null);
  const activeRef = useRef(false);
  const hide = useCallback(() => setState(null), []);
  const requestCancel = useCallback(() => {
    setState((current) => current ? { ...current, cancelRequested: true, progress: "正在取消，请稍候…" } : current);
  }, []);
  const run = useCallback(async <T,>(opts: BusyOpts, task: () => Promise<T>): Promise<T> => {
    if (activeRef.current) throw new Error("已有操作正在执行，请等待当前操作完成。");
    activeRef.current = true;
    setState({ ...opts, progress: undefined });
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
          setState((s) => (s ? { ...s, progress: isDone ? "完成" : e.payload } : s));
        });
      }
      return await task();
    }
    finally { un?.(); activeRef.current = false; setState(null); }
  }, []);
  return <BusyCtx.Provider value={{ state, run, hide, requestCancel }}>{children}</BusyCtx.Provider>;
}
/** 返回 run：await busy({title,...}, () => invoke(...))，期间弹模态挡操作。 */
export function useBusy() { return useContext(BusyCtx).run; }

export function BusyHost() {
  const { state, requestCancel } = useContext(BusyCtx);
  if (!state) return null;
  return (
    <div className="modalmask" style={{ zIndex: 200 }}>
      <div className="modal" style={{ minWidth: 380, maxWidth: 470 }}>
        <div className="modalhd"><span><i className="ti ti-loader spin" /> {state.title}</span></div>
        <div className="modalbody">
          {state.message && <div style={{ fontSize: 13, color: "var(--tx)", lineHeight: 1.7 }}>{state.message}</div>}
          {(state.progress || state.progressEvent) && (
            <div className="instbar trace-card" style={{ margin: "10px 0 0", overflow: "hidden", flexDirection: "column", alignItems: "stretch", gap: 8 }}>
              <span className="border-runner" aria-hidden="true" />
              <div style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0 }}>
              <i className="ti ti-loader spin" style={{ color: "var(--acc)" }} />
              <span className="ptxt" style={{ flex: 1, minWidth: 0, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }} title={state.progress ?? "正在处理"}>{state.progress ?? "正在处理…"}</span>
              </div>
            </div>
          )}
          <div style={{ fontSize: 11.5, color: "var(--mut)", marginTop: 10, lineHeight: 1.6 }}>
            请保持 Stacker 运行。操作完成后，此窗口会自动关闭。</div>
        </div>
        {state.cancel && (
          <div className="modalft">
            <button className="gh sm" disabled={state.cancelRequested} onClick={() => {
              requestCancel();
              state.cancel!.onCancel();
            }}>{state.cancelRequested ? "正在取消…" : state.cancel.label}</button>
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
  const modalRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  const titleId = useId();
  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);
  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const frame = window.requestAnimationFrame(() => {
      const target = modalRef.current?.querySelector<HTMLElement>("[autofocus]")
        ?? modalRef.current?.querySelector<HTMLElement>("button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex='-1'])");
      target?.focus();
    });
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && onCloseRef.current) {
        event.preventDefault();
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab" || !modalRef.current) return;
      const focusable = [...modalRef.current.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex='-1'])")];
      if (!focusable.length) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      window.cancelAnimationFrame(frame);
      document.removeEventListener("keydown", onKeyDown);
      previous?.focus();
    };
  }, []);
  return (
    <div className="modalmask">
      <div ref={modalRef} role="dialog" aria-modal="true" aria-labelledby={titleId} className={"modal" + (wide ? " wide" : "")}>
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
    <Modal title={title} icon={icon ?? (danger ? "ti-alert-triangle" : "ti-help-circle")} onClose={onClose}
      footer={<>
        <button className="gh sm" onClick={onClose} disabled={busy}>取消</button>
        <button className={"pr sm" + (danger ? " danger-solid" : "")} style={danger ? { background: "#d6463d" } : undefined}
          onClick={onConfirm} disabled={busy}>{confirmLabel}</button>
      </>}>
      <div style={{ fontSize: 13, lineHeight: 1.7, color: "var(--tx)" }}>{message}</div>
    </Modal>
  );
}
