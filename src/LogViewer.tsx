import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "./invoke";
import { AiAskModal, askAi } from "./features/ai/AiAsk";

type LogChunk = { path: string; content: string; offset: number; truncated: boolean };
type AppSettings = { log_level: "error" | "warn" | "info" | "debug" };

/** The newest errors and warnings in the log, oldest first. */
export function recentProblems(text: string, limit = 60): string[] {
  return text.split("\n").filter((line) => /\]\[(ERROR|WARN)\]\[/.test(line)).slice(-limit);
}

export default function LogViewer() {
  const [text, setText] = useState("");
  const [path, setPath] = useState("");
  const [level, setLevel] = useState("ERROR");
  const [error, setError] = useState("");
  const [asking, setAsking] = useState<string[] | null>(null);
  const offsetRef = useRef(0);
  const pathRef = useRef("");
  const viewRef = useRef<HTMLPreElement>(null);

  useEffect(() => {
    invoke<AppSettings>("settings_get")
      .then((settings) => setLevel((settings.log_level || "error").toUpperCase()))
      .catch(() => undefined);

    let disposed = false;
    let reading = false;
    const read = async () => {
      if (disposed || reading) return;
      reading = true;
      try {
        const chunk = await invoke<LogChunk>("settings_read_log", { offset: offsetRef.current });
        if (disposed) return;
        if (pathRef.current && pathRef.current !== chunk.path) {
          pathRef.current = chunk.path;
          offsetRef.current = 0;
          setPath(chunk.path);
          setText("");
          return;
        }
        pathRef.current = chunk.path;
        offsetRef.current = chunk.offset;
        setPath(chunk.path);
        setError("");
        if (chunk.content) {
          const prefix = chunk.truncated ? "[仅显示日志末尾内容]\n" : "";
          setText((current) => (current + prefix + chunk.content).slice(-600_000));
        }
      } catch (cause) {
        if (!disposed) setError(String(cause));
      } finally {
        reading = false;
      }
    };

    read();
    const timer = window.setInterval(read, 800);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    if (viewRef.current) viewRef.current.scrollTop = viewRef.current.scrollHeight;
  }, [text]);

  async function openLogsDir() {
    try {
      await invoke("settings_open_logs_dir");
    } catch (cause) {
      setError(String(cause));
    }
  }

  const problems = recentProblems(text);

  async function closeWindow() {
    await getCurrentWindow().close();
  }

  return (
    <div className="a log-window">
      <header className="log-window-head">
        <div className="log-window-title"><i className="ti ti-terminal-2" /><span>实时日志</span><span className="bd n">{level}</span></div>
        <div className="log-window-actions">
          <button className="gh sm" disabled={problems.length === 0} title={problems.length === 0 ? "当前日志里没有错误和警告" : undefined} onClick={() => setAsking(problems)}>
            <i className="ti ti-sparkles" /> AI 看错误
          </button>
          <button className="gh sm" onClick={openLogsDir}><i className="ti ti-folder-open" /> 打开日志目录</button>
          <button className="ib sm" title="关闭实时日志" aria-label="关闭实时日志" onClick={closeWindow}><i className="ti ti-x" /></button>
        </div>
      </header>
      <main className="log-window-main">
        <div className="live-log-meta">
          <span className="live-log-state"><i /> 实时读取</span>
          <span className="mono" title={path}>{path || "正在定位当天日志文件…"}</span>
        </div>
        {error && <div className="banner red"><i className="ti ti-alert-circle lead" /><div className="bt">读取日志失败：{error}</div></div>}
        <pre ref={viewRef} className="live-log-view">{text || "当前日志文件暂无记录。切换到 DEBUG 后执行操作，可查看更完整的诊断信息。"}</pre>
      </main>
      {asking && <AiAskModal title="最近的错误" sub={`${asking.length} 条错误和警告`} waiting="AI 正在看日志…" onClose={() => setAsking(null)}
        note="发给 AI 的是：日志里最近的错误和警告。看起来像密钥、令牌的内容会先遮掉。"
        run={() => askAi("app_log", { lines: asking.join("\n") })} />}
    </div>
  );
}
