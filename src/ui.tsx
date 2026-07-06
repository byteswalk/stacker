import { createContext, useContext, useState, useCallback, type ReactNode } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/* ───────────── Toast ───────────── */
type ToastKind = "ok" | "err" | "info";
type ToastItem = { id: number; msg: string; kind: ToastKind };
const ToastCtx = createContext<{ push: (msg: string, kind?: ToastKind) => void; items: ToastItem[] }>({
  push: () => {}, items: [],
});
let _tid = 0;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const push = useCallback((msg: string, kind: ToastKind = "ok") => {
    const id = ++_tid;
    setItems((t) => [...t, { id, msg, kind }]);
    setTimeout(() => setItems((t) => t.filter((x) => x.id !== id)), 3000);
  }, []);
  return <ToastCtx.Provider value={{ push, items }}>{children}</ToastCtx.Provider>;
}
export function useToast() { return useContext(ToastCtx).push; }

const TOAST_ICON: Record<ToastKind, string> = { ok: "ti-circle-check", err: "ti-alert-circle", info: "ti-info-circle" };
/** Render inside `.a` so the scoped `.a .toast` styles + CSS vars apply. */
export function ToastHost() {
  const { items } = useContext(ToastCtx);
  return (
    <div style={{ position: "fixed", left: "50%", bottom: 18, transform: "translateX(-50%)", zIndex: 100, display: "flex", flexDirection: "column", gap: 8, alignItems: "center", pointerEvents: "none" }}>
      {items.map((t) => (
        <div key={t.id} className={"toast " + t.kind} style={{ position: "static", transform: "none", pointerEvents: "auto" }}>
          <i className={"ti " + TOAST_ICON[t.kind]} /> {t.msg}
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
type BusyState = (BusyOpts & { progress?: string }) | null;
const BusyCtx = createContext<{
  state: BusyState;
  run: <T>(opts: BusyOpts, task: () => Promise<T>) => Promise<T>;
  hide: () => void;
}>({ state: null, run: async (_o, t) => t(), hide: () => {} });

export function BusyProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<BusyState>(null);
  const hide = useCallback(() => setState(null), []);
  const run = useCallback(async <T,>(opts: BusyOpts, task: () => Promise<T>): Promise<T> => {
    setState({ ...opts, progress: undefined });
    let un: UnlistenFn | undefined;
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
    try { return await task(); }
    finally { un?.(); setState(null); }
  }, []);
  return <BusyCtx.Provider value={{ state, run, hide }}>{children}</BusyCtx.Provider>;
}
/** 返回 run：await busy({title,...}, () => invoke(...))，期间弹模态挡操作。 */
export function useBusy() { return useContext(BusyCtx).run; }

export function BusyHost() {
  const { state } = useContext(BusyCtx);
  if (!state) return null;
  return (
    <div className="modalmask" style={{ zIndex: 200 }}>
      <div className="modal" style={{ minWidth: 380, maxWidth: 470 }}>
        <div className="modalhd"><span><i className="ti ti-loader spin" /> {state.title}</span></div>
        <div className="modalbody">
          {state.message && <div style={{ fontSize: 13, color: "var(--tx)", lineHeight: 1.7 }}>{state.message}</div>}
          {state.progress && (
            <div className="instbar" style={{ margin: "10px 0 0", overflow: "hidden" }}>
              <i className="ti ti-loader spin" style={{ color: "var(--acc)" }} />
              <span className="ptxt" style={{ flex: 1, minWidth: 0, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }} title={state.progress}>{state.progress}</span></div>
          )}
          <div style={{ fontSize: 11.5, color: "var(--mut)", marginTop: 10, lineHeight: 1.6 }}>
            请等待当前操作完成；完成后会自动刷新状态并关闭窗口。</div>
        </div>
        {state.cancel && (
          <div className="modalft">
            <button className="gh sm" onClick={() => { state.cancel!.onCancel(); }}>{state.cancel.label}</button>
          </div>
        )}
      </div>
    </div>
  );
}

/* ───────────── 加载占位（统一的"检测中"动画） ───────────── */
export function Loading({ text }: { text: string }) {
  return (
    <div className="stub">
      <div className="si"><i className="ti ti-loader spin" /></div>
      <p>{text}</p>
    </div>
  );
}

/* ───────────── Modal ───────────── */
export function Modal({ title, icon, sub, wide, children, footer, onClose }: {
  title: ReactNode; icon?: string; sub?: ReactNode; wide?: boolean;
  children?: ReactNode; footer?: ReactNode; onClose?: () => void;
}) {
  return (
    <div className="modalmask" onClick={(e) => { if (e.target === e.currentTarget) onClose?.(); }}>
      <div className={"modal" + (wide ? " wide" : "")}>
        <div className="modalhd">
          <span>{icon && <i className={"ti " + icon} />} {title}</span>
          {onClose && <button className="ic" onClick={onClose}><i className="ti ti-x" /></button>}
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
