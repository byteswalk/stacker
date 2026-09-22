import { useCallback, useEffect, useState, type ReactNode } from "react";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { useBusy, useToast } from "../../ui";
import { FoldToggle, useFold } from "./Fold";
import { shortVersion } from "../agents/tileState";

type LoginStatus = { state: "logged_in" | "logged_out" | "unknown"; method: string };
type AgentModel = { call: string; label: string; efforts: string[]; defaultEffort: string | null };
export type AgentCard = {
  id: string; name: string; installed: boolean; version: string | null;
  supported: boolean; reason: string; login: LoginStatus | null; enabled: boolean;
  defaultModel: string | null; defaultEffort: string | null; efforts: string[]; models: AgentModel[];
};
type TestResult = { ok: boolean; reply: string; error: string; elapsedMs: number; model: string; effort: string };

const LOGIN: Record<LoginStatus["state"], { label: string; cls: string }> = {
  logged_in: { label: "已登录", cls: "g" },
  logged_out: { label: "未登录", cls: "y" },
  unknown: { label: "登录状态未知", cls: "n" },
};

/** Agents whose defaults follow the summary settings; the rest use their CLI's own defaults. */
const SUMMARY_AGENTS = ["codex", "claude"];

const RUN_ERRORS: Record<string, string> = {
  E_RUNNER_AUTH: "未登录：请在终端运行该智能体并完成登录。",
  E_RUNNER_MISSING: "未找到命令行程序。",
  E_RUNNER_TIMEOUT: "5 分钟内没有回复。",
  E_PROMPT_TOO_LONG: "内容太长：这个智能体只能从命令行接收提问，上限约 3 万字。",
};

const CACHE_KEY = "stacker.gateway.agents.v1";

/** The last check, shown at once while a fresh one runs (each check starts every agent CLI). */
function cachedCards(): AgentCard[] | null {
  try { return JSON.parse(localStorage.getItem(CACHE_KEY) || "null") as AgentCard[] | null; } catch { return null; }
}

function saveCards(cards: AgentCard[]) {
  try { localStorage.setItem(CACHE_KEY, JSON.stringify(cards)); } catch { /* per-viewer convenience only */ }
}

/** One block per agent: can it be used, is it signed in, which models and efforts, on/off, test. */
export function GatewayAgents() {
  const { tr: t } = useI18n();
  const toast = useToast();
  const busy = useBusy();
  const [cards, setCards] = useState<AgentCard[] | null>(cachedCards);
  const [refreshing, setRefreshing] = useState(false);
  const [tests, setTests] = useState<Record<string, TestResult | "running">>({});

  // No blocking dialog: the last result stays on screen and is replaced when the check ends.
  const load = useCallback(async () => {
    setRefreshing(true);
    try {
      const next = await invoke<AgentCard[]>("gateway_agents");
      setCards(next);
      saveCards(next);
    } finally {
      setRefreshing(false);
    }
  }, []);
  useEffect(() => { load().catch(() => setCards((old) => old ?? [])); }, [load]);

  async function toggle(card: AgentCard, enabled: boolean) {
    try { await invoke("gateway_set_agent", { agent: card.id, enabled }); await load(); }
    catch (e) { toast(String(e), "err"); }
  }
  async function test(card: AgentCard, model?: string) {
    const key = model ?? card.id;
    setTests((old) => ({ ...old, [key]: "running" }));
    try {
      const result = await busy({ title: `${t("正在测试")} ${card.name}`, message: t("发送一条简短消息并等待回复，通常需要 5–30 秒。") },
        () => invoke<TestResult>("gateway_test", { agent: card.id, model: model ? model.slice(card.id.length + 1) : null }));
      setTests((old) => ({ ...old, [key]: result }));
    } catch (e) {
      setTests((old) => ({ ...old, [key]: { ok: false, reply: "", error: String(e), elapsedMs: 0, model: "", effort: "" } }));
    }
  }
  const copy = (text: string) => { void navigator.clipboard.writeText(text).then(() => toast(t("已复制"), "ok")); };

  const testLine = (key: string) => {
    const r = tests[key];
    if (!r) return null;
    if (r === "running") return <span className="gw-test"><i className="ti ti-loader spin" /> {t("测试中…")}</span>;
    return r.ok
      ? <span className="gw-test ok"><i className="ti ti-circle-check" /> {t("可用")} · {(r.elapsedMs / 1000).toFixed(1)}s · {r.model || t("CLI 默认模型")} · {r.effort || t("CLI 默认推理")}</span>
      : <span className="gw-test bad"><i className="ti ti-alert-circle" /> {t(RUN_ERRORS[r.error] ?? r.error)}</span>;
  };

  if (!cards) return <div className="pxcard"><p className="proxy-note"><i className="ti ti-loader spin" /> {t("正在检查本机智能体…")}</p></div>;
  const refreshNote = refreshing && <p className="proxy-note gw-refreshing"><i className="ti ti-loader spin" /> {t("正在更新登录状态和模型列表…")}</p>;
  const usable = cards.filter((c) => c.supported);
  const others = cards.filter((c) => !c.supported);

  return <>
    {refreshNote}
    {usable.map((card) => <AgentBlock key={card.id} card={card} testLine={testLine} running={(key) => tests[key] === "running"}
      onTest={(model) => void test(card, model)} onToggle={(on) => void toggle(card, on)} onCopy={copy} />)}

    {!!others.length && <div className="pxcard">
      <div className="pxsec"><i className="ti ti-robot-off" /> {t("暂不能通过接口服务使用")}</div>
      <div className="gw-others">
        {others.map((c) => <div key={c.id}><b>{c.name}</b>{c.version && <span className="s dim">v{shortVersion(c.version)}</span>}<span className="s dim">{t(c.reason)}</span></div>)}
      </div>
    </div>}
  </>;
}

/** One agent: a header row that is always visible, and its defaults and models when opened. */
function AgentBlock({ card, testLine, running, onTest, onToggle, onCopy }: {
  card: AgentCard;
  testLine: (key: string) => ReactNode;
  running: (key: string) => boolean;
  onTest: (model?: string) => void;
  onToggle: (enabled: boolean) => void;
  onCopy: (text: string) => void;
}) {
  const { tr: t } = useI18n();
  const [open, toggleOpen] = useFold(`agent:${card.id}`, false);
  const login = LOGIN[card.login?.state ?? "unknown"];
  return <div className={"pxcard gw-agent" + (open ? " open" : "")}>
        <div className="gw-agent-head">
          <FoldToggle open={open} onToggle={toggleOpen} label={t(open ? "收起模型列表" : "展开模型列表")} />
          <b className="gw-agent-name" onClick={toggleOpen}>{card.name}</b>
          {card.version && <span className="s dim">v{shortVersion(card.version)}</span>}
          <span className={"bd " + login.cls}>{t(login.label)}{card.login?.method ? ` · ${card.login.method}` : ""}</span>
          <div className="gw-agent-actions">
            {testLine(card.id)}
            <button className="gh sm" disabled={running(card.id)} onClick={() => onTest()}><i className="ti ti-player-play" /> {t("测试")}</button>
            <label className="gw-switch" title={t("关闭后接口服务不再接受这个智能体的请求")}>
              <span>{t(card.enabled ? "已开放" : "已关闭")}</span>
              <span className="sw"><input type="checkbox" checked={card.enabled} onChange={(e) => onToggle(e.target.checked)} /><span className="tk" /></span>
            </label>
          </div>
        </div>
        {!open && <p className="proxy-note gw-summary" onClick={toggleOpen}>
          {t("默认")}：<b>{card.defaultModel ?? t("CLI 默认模型")}</b> · <b>{card.defaultEffort ?? t("CLI 默认推理")}</b> · {card.models.length} {t("个可指定的模型")}
        </p>}
        {open && <>
        <p className="proxy-note">
          {t("请求里写")} <code>{card.id}</code> {t("时使用默认")}：<b>{card.defaultModel ?? t("CLI 默认模型")}</b> · <b>{card.defaultEffort ?? t("CLI 默认推理")}</b>{SUMMARY_AGENTS.includes(card.id) ? t("（在「会话数据 → 设置 → 摘要」中修改）") : ""}{t("。也可以写下表中的调用名指定模型，并用 reasoning_effort 指定推理档位（Anthropic 风格请求用默认档位）。")}
        </p>
        {!!card.efforts.length && <p className="proxy-note gw-efforts">{t("只写智能体名时可用的推理档位")}：{card.efforts.map((e) => <span key={e} className={"chip" + (e === card.defaultEffort ? " auto" : "")}>{e}</span>)}</p>}
        <div className="gw-models">
          <div className="gw-model head"><span>{t("调用名")}</span><span>{t("模型")}</span><span>{t("可用推理档位")}</span><span /></div>
          {card.models.map((m) => <div className="gw-model" key={m.call}>
            <span><code>{m.call}</code> <button className="ic" title={t("复制")} onClick={() => onCopy(m.call)}><i className="ti ti-copy" /></button></span>
            <span>{m.label}</span>
            <span className="gw-efforts">{m.efforts.map((e) => <span key={e} className={"chip" + (e === m.defaultEffort ? " auto" : "")} title={e === m.defaultEffort ? t("该模型的默认档位") : undefined}>{e}</span>)}</span>
            <span className="gw-model-test">{testLine(m.call)}<button className="gh sm" disabled={running(m.call)} onClick={() => onTest(m.call)}>{t("测试")}</button></span>
          </div>)}
          {!card.models.length && <p className="proxy-note">{t("没有读到模型列表，可在请求里直接写完整模型名。")}</p>}
        </div>
        </>}
      </div>;

}
