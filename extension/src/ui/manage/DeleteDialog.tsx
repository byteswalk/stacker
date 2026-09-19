import { useRef, useState } from "react";
import { t } from "../../i18n";
import type { Conversation } from "../../lib/db";
import type { DeleteMode, ItemResult } from "../../lib/deleteJob";
import { errorText } from "../errors";

const MODES: { value: DeleteMode; label: string; hint: string }[] = [
  { value: "slim", label: "精简导出后删除", hint: "把用户和助手的正文存成 Markdown，再删除网站上的对话。" },
  { value: "full", label: "完整备份后删除", hint: "保存完整 Markdown 和原始数据，再删除网站上的对话。" },
  { value: "direct", label: "直接删除", hint: "不留任何副本。" },
];

export function DeleteDialog({ items, currentAccounts, onRun, onClose }: {
  items: Conversation[]; currentAccounts: Set<string>;
  onRun: (mode: DeleteMode, signal: AbortSignal, onProgress: (r: ItemResult[]) => void) => Promise<ItemResult[]>;
  onClose: (changed: boolean) => void;
}) {
  const [mode, setMode] = useState<DeleteMode>("slim");
  const [confirming, setConfirming] = useState(false);
  const [results, setResults] = useState<ItemResult[] | null>(null);
  const [running, setRunning] = useState(false);
  const controller = useRef<AbortController | null>(null);
  const others = items.filter((c) => !currentAccounts.has(c.account)).length;

  async function start() {
    if (mode === "direct" && !confirming) { setConfirming(true); return; }
    controller.current = new AbortController();
    setRunning(true);
    try { setResults(await onRun(mode, controller.current.signal, setResults)); } finally { setRunning(false); }
  }

  const done = results?.filter((r) => r.status === "done").length ?? 0;
  return <div className="mask"><div className="dialog" role="dialog" aria-modal="true">
    <h3>{t("删除对话")}</h3>
    {!results && <>
      <p>{t("将删除")} {items.length} {t("条对话。")}</p>
      {others > 0 && <p className="warn">{others} {t("条不属于当前登录的账号，会被跳过。请先在网站上切换到对应账号。")}</p>}
      {MODES.map((m) => <label key={m.value} style={{ display: "block", margin: "6px 0" }}>
        <input type="radio" name="mode" value={m.value} checked={mode === m.value} onChange={() => { setMode(m.value); setConfirming(false); }} /> <b>{t(m.label)}</b>
        <div className="mut">{t(m.hint)}</div>
      </label>)}
      {confirming && <p className="err">{t("直接删除不会留下任何副本，删除后无法恢复。确定继续吗？")}</p>}
      <div className="row">
        <button onClick={() => onClose(false)}>{t("取消")}</button>
        <button className="danger" onClick={() => void start()}>{confirming ? t("确认直接删除") : `${t("删除")} ${items.length} ${t("条")}`}</button>
      </div>
    </>}
    {results && <>
      <p>{running ? t("正在删除，请不要关闭此页…") : t("已完成")} {done} / {results.length}</p>
      <ul className="list">{results.map((r) => <li key={r.key} style={{ cursor: "default" }}>
        <span>{r.status === "done" ? "✓" : r.status === "pending" ? "…" : "✗"}</span>
        <span>{r.title}</span>
        <span className="mut">{r.error ? errorText(r.error) : ""}</span>
      </li>)}</ul>
      <div className="row">
        {running && <button onClick={() => controller.current?.abort()}>{t("中止")}</button>}
        <button className="primary" disabled={running} onClick={() => onClose(true)}>{t("完成")}</button>
      </div>
    </>}
  </div></div>;
}
