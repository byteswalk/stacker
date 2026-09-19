import { useEffect, useRef, useState } from "react";
import { t } from "../../i18n";
import type { Conversation } from "../../lib/db";
import type { DeleteMode, ItemResult } from "../../lib/deleteJob";
import type { SiteId } from "../../shared/types";
import { SITES } from "../../sites/registry";
import { errorText } from "../errors";

const MODES: { value: DeleteMode; label: string; hint: string }[] = [
  { value: "slim", label: "精简导出后删除", hint: "把用户和助手的正文存成 Markdown，再删除网站上的对话。" },
  { value: "full", label: "完整备份后删除", hint: "保存当前分支的完整 Markdown（含工具消息和附件名），并把同样的消息另存为 JSON，再删除网站上的对话。" },
  { value: "direct", label: "直接删除", hint: "不留任何副本。" },
];

/** One line per site and account: how many chosen items, and why they will not be deleted if so. */
interface Group { site: SiteId; account: string; count: number; problem: string; blocked: boolean }

export function DeleteDialog({ items, currentAccounts, aliasOf, siteErrors, onRun, onClose }: {
  items: Conversation[]; currentAccounts: Set<string>; aliasOf: (account: string) => string;
  /** Sites that cannot be deleted from now (account check failed or interface changed), with the reason shown. */
  siteErrors: Partial<Record<SiteId, string>>;
  onRun: (items: Conversation[], mode: DeleteMode, signal: AbortSignal, onProgress: (r: ItemResult[]) => void) => Promise<ItemResult[]>;
  onClose: (changed: boolean) => void;
}) {
  const [mode, setMode] = useState<DeleteMode>("slim");
  const [confirming, setConfirming] = useState(false);
  const [results, setResults] = useState<ItemResult[] | null>(null);
  const [running, setRunning] = useState(false);
  const controller = useRef<AbortController | null>(null);
  const runningRef = useRef(false);
  const runnable = items.filter((c) => !siteErrors[c.site]);
  const willDelete = runnable.filter((c) => currentAccounts.has(c.account)).length;
  const others = runnable.length - willDelete;
  const groups = new Map<string, Group>();
  for (const c of items) {
    const id = `${c.site}|${c.account}`;
    const siteError = siteErrors[c.site];
    const problem = siteError ?? (currentAccounts.has(c.account) ? "" : t("不是当前登录的账号，会被跳过"));
    const g = groups.get(id) ?? { site: c.site, account: c.account, count: 0, problem, blocked: !!siteError };
    g.count++;
    groups.set(id, g);
  }

  useEffect(() => {
    if (!running) return;
    const ask = (e: BeforeUnloadEvent) => e.preventDefault();
    window.addEventListener("beforeunload", ask);
    return () => window.removeEventListener("beforeunload", ask);
  }, [running]);

  async function start() {
    if (mode === "direct" && !confirming) { setConfirming(true); return; }
    if (runningRef.current) return;
    runningRef.current = true;
    controller.current = new AbortController();
    setResults(runnable.map((c) => ({ key: c.key, title: c.title, status: "pending", error: "" })));
    setRunning(true);
    try { setResults(await onRun(runnable, mode, controller.current.signal, setResults)); } finally { setRunning(false); runningRef.current = false; }
  }

  const done = results?.filter((r) => r.status === "done").length ?? 0;
  return <div className="mask"><div className="dialog" role="dialog" aria-modal="true">
    <h3>{t("删除对话")}</h3>
    {!results && <>
      <p>{t("将删除")} {willDelete} {t("条对话。")}</p>
      <ul className="groups">{[...groups.values()].map((g) => <li key={`${g.site}:${g.account}`}>
        {SITES[g.site].label} · {aliasOf(g.account)}：{g.count} {t("条")}
        {g.problem && <span className={g.blocked ? "err" : "warn"}> — {g.problem}</span>}
      </li>)}</ul>
      {others > 0 && <p className="warn">{others} {t("条不属于当前登录的账号，会被跳过。请先在网站上切换到对应账号。")}</p>}
      {MODES.map((m) => <label key={m.value} style={{ display: "block", margin: "6px 0" }}>
        <input type="radio" name="mode" value={m.value} checked={mode === m.value} disabled={running} onChange={() => { setMode(m.value); setConfirming(false); }} /> <b>{t(m.label)}</b>
        <div className="mut">{t(m.hint)}</div>
      </label>)}
      {confirming && <p className="err">{t("直接删除不会留下任何副本，删除后无法恢复。确定继续吗？")}</p>}
      <div className="row">
        <button disabled={running} onClick={() => onClose(false)}>{t("取消")}</button>
        <button className="danger" disabled={running || willDelete === 0} onClick={() => void start()}>{confirming ? t("确认直接删除") : `${t("删除")} ${willDelete} ${t("条")}`}</button>
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
