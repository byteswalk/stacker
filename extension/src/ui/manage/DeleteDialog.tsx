import { CheckCircleFilled, CloseCircleFilled, LoadingOutlined } from "@ant-design/icons";
import { Alert, Button, Flex, Modal, Progress, Radio, Typography } from "antd";
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
  const settled = results?.filter((r) => r.status !== "pending").length ?? 0;

  const footer = results
    ? <Flex justify="flex-end" gap={8}>
      {running && <Button onClick={() => controller.current?.abort()}>{t("中止")}</Button>}
      <Button type="primary" disabled={running} onClick={() => onClose(true)}>{t("完成")}</Button>
    </Flex>
    : <Flex justify="flex-end" gap={8}>
      <Button disabled={running} onClick={() => onClose(false)}>{t("取消")}</Button>
      <Button type="primary" danger disabled={running || willDelete === 0} onClick={() => void start()}>
        {confirming ? t("确认直接删除") : `${t("删除")} ${willDelete} ${t("条")}`}
      </Button>
    </Flex>;

  return <Modal
    open
    title={t("删除对话")}
    width={620}
    mask={{ closable: false }}
    keyboard={false}
    closable={!running && !results}
    onCancel={() => { if (!running) onClose(!!results); }}
    footer={footer}
  >
    {!results && <>
      <p>{t("将删除")} {willDelete} {t("条对话。")}</p>
      <ul className="groups">{[...groups.values()].map((g) => <li key={`${g.site}:${g.account}`}>
        {SITES[g.site].label} · {aliasOf(g.account)}：{g.count} {t("条")}
        {g.problem && <Typography.Text type={g.blocked ? "danger" : "warning"}> — {g.problem}</Typography.Text>}
      </li>)}</ul>
      {others > 0 && <Alert
        type="warning" showIcon style={{ marginBottom: 12 }}
        title={`${others} ${t("条不属于当前登录的账号，会被跳过。请先在网站上切换到对应账号。")}`}
      />}
      <Radio.Group
        value={mode} disabled={running}
        onChange={(e) => { setMode(e.target.value as DeleteMode); setConfirming(false); }}
        style={{ display: "flex", flexDirection: "column", gap: 8, width: "100%" }}
      >
        {MODES.map((m) => <Radio key={m.value} value={m.value} className="mode-card">
          <b>{t(m.label)}</b>
          <div className="dim">{t(m.hint)}</div>
        </Radio>)}
      </Radio.Group>
      {confirming && <Alert
        type="error" showIcon style={{ marginTop: 12 }}
        title={t("直接删除不会留下任何副本，删除后无法恢复。确定继续吗？")}
      />}
    </>}
    {results && <>
      <Flex align="center" gap={10} style={{ marginBottom: 10 }}>
        <Typography.Text>{running ? t("正在删除，请不要关闭此页…") : t("已完成")} {done} / {results.length}</Typography.Text>
      </Flex>
      <Progress
        percent={results.length ? Math.round((settled / results.length) * 100) : 100}
        status={running ? "active" : "normal"}
        size="small"
      />
      <ul className="results">{results.map((r) => <li key={r.key}>
        <span className="icon">{r.status === "done"
          ? <CheckCircleFilled style={{ color: "#6bcf86" }} />
          : r.status === "pending" ? <LoadingOutlined /> : <CloseCircleFilled style={{ color: "#e2625b" }} />}</span>
        <span className="text">{r.title}</span>
        <Typography.Text type="secondary">{r.error ? errorText(r.error) : ""}</Typography.Text>
      </li>)}</ul>
    </>}
  </Modal>;
}
