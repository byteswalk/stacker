import { useCallback, useEffect, useState, type ReactNode } from "react";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { Select } from "../../Select";
import { useBusy, useToast } from "../../ui";
import { FoldToggle, useFold } from "./Fold";
import { shortVersion } from "../agents/tileState";

type LoginStatus = { state: "logged_in" | "logged_out" | "unknown"; method: string };
type AgentModel = { call: string; label: string; efforts: string[]; defaultEffort: string | null };
export type AgentCard = {
  id: string; name: string; vendor: string; installed: boolean; version: string | null;
  supported: boolean; reason: string; login: LoginStatus | null; enabled: boolean;
  defaultModel: string | null; defaultEffort: string | null; defaultIsChosen: boolean;
  efforts: string[]; models: AgentModel[];
};
type TestResult = { ok: boolean; reply: string; error: string; detail?: string; elapsedMs: number; model: string; effort: string };

/** How to sign in, in the user's own terminal. */
const LOGIN_COMMAND: Record<string, string> = {
  codex: "codex",
  claude: "claude",
  agy: "agy",
  kimi: "kimi login",
  mimo: "mimo auth login",
  qoder: "qoder login",
  qodercn: "qodercn login",
  codebuddy: "codebuddy",
};

const RUN_ERRORS: Record<string, string> = {
  E_RUNNER_AUTH: "未登录：请在终端运行该智能体并完成登录。",
  E_RUNNER_INELIGIBLE: "账号已登录，但厂商不允许它使用：换一个有资格的账号登录，或先按下方提示验证账号。",
  E_RUNNER_MISSING: "未找到命令行程序。",
  E_RUNNER_TIMEOUT: "5 分钟内没有回复。",
  E_PROMPT_TOO_LONG: "内容太长：这个智能体只能从命令行接收提问，上限约 3 万字。",
};

const CACHE_KEY = "stacker.gateway.agents.v1";
const TESTS_KEY = "stacker.gateway.tests.v1";
/** Models shown before the list is expanded; more than this is a wall of names. */
const SHOWN_MODELS = 5;

type PastTest = { at: number; ok: boolean; elapsedMs: number };

function readJson<T>(key: string): T | null {
  try { return JSON.parse(localStorage.getItem(key) || "null") as T | null; } catch { return null; }
}

function writeJson(key: string, value: unknown) {
  try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* per-viewer convenience only */ }
}

/** "刚刚" / "13 分钟前" / "3 小时前": how fresh the last successful test is. */
/** The link a CLI printed with its refusal, for the button that opens it. */
export function detailLink(detail: string): string | null {
  return detail.split(/\s+/).find((word) => word.startsWith("https://")) ?? null;
}

/** The same message without the link, which the button carries instead. */
export function detailText(detail: string): string {
  const link = detailLink(detail);
  return (link ? detail.replace(link, "") : detail).trim();
}

export function sinceText(at: number, now = Date.now()): string {
  const minutes = Math.floor((now - at) / 60_000);
  if (minutes < 1) return "刚刚";
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  return `${Math.floor(hours / 24)} 天前`;
}

/**
 * One line that says whether the agent can be used, instead of a sign-in state and a test
 * result contradicting each other: a test that went through proves it works.
 */
export function agentState(card: AgentCard, past?: PastTest): { label: string; cls: string } {
  if (past?.ok) return { label: `可用 · ${sinceText(past.at)}测试通过`, cls: "g" };
  if (card.login?.state === "logged_out") return { label: "未登录", cls: "y" };
  if (card.login?.state === "logged_in") return { label: "已登录", cls: "g" };
  return { label: "未测试", cls: "n" };
}

/**
 * What a model's reasoning levels add to the line above the table, which already names the
 * ones they all share: nothing when they match, "仅 …" when the model takes fewer.
 */
export function effortNote(own: string[], shared: string[]): string {
  if (!own.length || !shared.length) return "";
  const set = new Set(shared);
  const same = own.length === shared.length && own.every((e) => set.has(e));
  if (same) return "";
  return own.every((e) => set.has(e)) ? `仅 ${own.join(" / ")}` : `支持 ${own.join(" / ")}`;
}

/** One block per agent: can it be used, what it runs by default, how to call it, its models. */
export function GatewayAgents({ base, token }: { base: string; token: string }) {
  const { tr: t } = useI18n();
  const toast = useToast();
  const busy = useBusy();
  const [cards, setCards] = useState<AgentCard[] | null>(() => readJson<AgentCard[]>(CACHE_KEY));
  const [refreshing, setRefreshing] = useState(false);
  const [tests, setTests] = useState<Record<string, TestResult | "running">>({});
  const [past, setPast] = useState<Record<string, PastTest>>(() => readJson<Record<string, PastTest>>(TESTS_KEY) ?? {});

  // No blocking dialog: the last check stays on screen and is replaced when a new one ends.
  const load = useCallback(async () => {
    setRefreshing(true);
    try {
      const next = await invoke<AgentCard[]>("gateway_agents");
      setCards(next);
      writeJson(CACHE_KEY, next);
    } finally {
      setRefreshing(false);
    }
  }, []);
  useEffect(() => { load().catch(() => setCards((old) => old ?? [])); }, [load]);

  async function toggle(card: AgentCard, enabled: boolean) {
    try { await invoke("gateway_set_agent", { agent: card.id, enabled }); await load(); }
    catch (e) { toast(String(e), "err"); }
  }

  async function setDefault(card: AgentCard, model: string | null, effort: string | null) {
    try {
      await invoke("gateway_set_agent_default", { agent: card.id, model, effort });
      await load();
      toast(t("已保存默认设置"), "ok");
    } catch (e) { toast(String(e), "err"); }
  }

  async function test(card: AgentCard, model?: string) {
    const key = model ?? card.id;
    setTests((old) => ({ ...old, [key]: "running" }));
    try {
      const result = await busy({ title: `${t("正在测试")} ${card.name}`, message: t("发送一条简短消息并等待回复，通常需要 5–30 秒。") },
        () => invoke<TestResult>("gateway_test", { agent: card.id, model: model ? model.slice(card.id.length + 1) : null }));
      setTests((old) => ({ ...old, [key]: result }));
      setPast((old) => {
        const next = { ...old, [key]: { at: Date.now(), ok: result.ok, elapsedMs: result.elapsedMs } };
        writeJson(TESTS_KEY, next);
        return next;
      });
    } catch (e) {
      setTests((old) => ({ ...old, [key]: { ok: false, reply: "", error: String(e), detail: "", elapsedMs: 0, model: "", effort: "" } }));
    }
  }

  const copy = (text: string) => { void navigator.clipboard.writeText(text).then(() => toast(t("已复制"), "ok")); };

  const testLine = (key: string) => {
    const r = tests[key];
    if (!r) return null;
    if (r === "running") return <span className="gw-test"><i className="ti ti-loader spin" /> {t("测试中…")}</span>;
    if (r.ok) return <span className="gw-test ok"><i className="ti ti-circle-check" /> {(r.elapsedMs / 1000).toFixed(1)}s {t("测试通过")}</span>;
    return <span className="gw-test bad" title={r.detail || undefined}>
      <i className="ti ti-alert-circle" /> {t(RUN_ERRORS[r.error] ?? r.error)}
      {r.detail && <em className="gw-test-detail" title={r.detail}>{detailText(r.detail)}{detailLink(r.detail) && <button className="lk" onClick={() => void invoke("app_open_url", { url: detailLink(r.detail as string) })}>{t("去处理")}</button>}</em>}
    </span>;
  };

  if (!cards) return <div className="pxcard"><p className="proxy-note"><i className="ti ti-loader spin" /> {t("正在检查本机智能体…")}</p></div>;
  const refreshNote = refreshing && <p className="proxy-note gw-refreshing"><i className="ti ti-loader spin" /> {t("正在更新登录状态和模型列表…")}</p>;
  const usable = cards.filter((c) => c.supported);
  // Agents Stacker cannot drive are not the user's problem and stay off the page; a
  // signed-out one is one sign-in away from working, so that one still shows.
  const needLogin = cards.filter((c) => !c.supported && !!c.login);

  return <>
    {refreshNote}
    {usable.map((card) => <AgentBlock key={card.id} card={card} base={base} token={token}
      past={past} testLine={testLine} running={(key) => tests[key] === "running"}
      onTest={(model) => void test(card, model)} onToggle={(on) => void toggle(card, on)}
      onDefault={(model, effort) => void setDefault(card, model, effort)} onCopy={copy} />)}

    {!!needLogin.length && <div className="pxcard">
      <div className="pxsec"><i className="ti ti-login" /> {t("登录后即可用于接口服务")}</div>
      <div className="gw-others">
        {needLogin.map((c) => <div key={c.id}>
          <b>{c.name}</b>{c.version && <span className="s dim">v{shortVersion(c.version)}</span>}
          <span className="s dim">{t("在终端运行")} <code>{LOGIN_COMMAND[c.id] ?? c.id}</code> {t("完成登录")}</span>
        </div>)}
      </div>
    </div>}
  </>;
}

type Snippet = "openai" | "anthropic" | "curl" | "client";

/** Ready-to-run code for this agent, with the port, key and model already filled in. */
export function snippet(kind: Snippet, base: string, token: string, model: string): string {
  if (kind === "openai") {
    return `from openai import OpenAI\n\nclient = OpenAI(\n    base_url="${base}/v1",\n    api_key="${token}",\n)\nreply = client.chat.completions.create(\n    model="${model}",\n    messages=[{"role": "user", "content": "你好"}],\n)\nprint(reply.choices[0].message.content)`;
  }
  if (kind === "anthropic") {
    return `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="${base}",\n    api_key="${token}",\n)\nreply = client.messages.create(\n    model="${model}", max_tokens=1024,\n    messages=[{"role": "user", "content": "你好"}],\n)\nprint(reply.content[0].text)`;
  }
  if (kind === "curl") {
    return `curl ${base}/v1/chat/completions \\\n  -H "Authorization: Bearer ${token}" \\\n  -H "Content-Type: application/json" \\\n  -d '{"model":"${model}","messages":[{"role":"user","content":"你好"}]}'`;
  }
  return `接口类型：OpenAI 兼容\nAPI 地址：${base}/v1\nAPI 密钥：${token}\n模型名称：${model}`;
}

const SNIPPETS: [Snippet, string][] = [
  ["openai", "OpenAI 兼容"],
  ["anthropic", "Anthropic 兼容"],
  ["curl", "curl"],
  ["client", "客户端填写"],
];

function AgentBlock({ card, base, token, past, testLine, running, onTest, onToggle, onDefault, onCopy }: {
  card: AgentCard;
  base: string;
  token: string;
  past: Record<string, PastTest>;
  testLine: (key: string) => ReactNode;
  running: (key: string) => boolean;
  onTest: (model?: string) => void;
  onToggle: (enabled: boolean) => void;
  onDefault: (model: string | null, effort: string | null) => void;
  onCopy: (text: string) => void;
}) {
  const { tr: t } = useI18n();
  const [open, toggleOpen] = useFold(`agent:${card.id}`, false);
  const [kind, setKind] = useState<Snippet>("openai");
  const [query, setQuery] = useState("");
  const [all, setAll] = useState(false);
  const state = agentState(card, past[card.id]);
  const method = card.login?.state === "logged_in" && card.login.method ? card.login.method : "";
  const subtitle = [card.vendor, method, `${card.models.length} ${t("个模型")}`].filter(Boolean).join(" · ");
  // Efforts every model shares are said once above the table; a row only names its own.
  const shared = card.efforts;
  const needle = query.trim().toLowerCase();
  const matches = card.models.filter((m) => !needle
    || m.label.toLowerCase().includes(needle)
    || m.call.toLowerCase().includes(needle));
  const shown = all || needle ? matches : matches.slice(0, SHOWN_MODELS);
  const modelOptions = [{ value: "", label: t("跟随 CLI 默认") }, ...card.models.map((m) => ({ value: m.call.slice(card.id.length + 1), label: m.label }))];
  const effortOptions = [{ value: "", label: t("跟随 CLI 默认") }, ...shared.map((e) => ({ value: e, label: e }))];

  return <div className={"pxcard gw-agent" + (open ? " open" : "")}>
    <div className="gw-head">
      <FoldToggle open={open} onToggle={toggleOpen} label={t(open ? "收起" : "展开")} />
      <div className="gw-title" onClick={toggleOpen}>
        <b>{card.name} {card.version && <span className="s dim">v{shortVersion(card.version)}</span>}</b>
        <span className="s dim">{subtitle}</span>
      </div>
      <span className={"bd " + state.cls}>{t(state.label)}</span>
      <div className="gw-agent-actions">
        {testLine(card.id)}
        <button className="gh sm" disabled={running(card.id)} onClick={() => onTest()}><i className="ti ti-player-play" /> {t("测试")}</button>
        <label className="gw-switch" title={t("关闭后接口服务不再接受这个智能体的请求")}>
          <span>{t(card.enabled ? "已开放" : "已关闭")}</span>
          <span className="sw"><input type="checkbox" checked={card.enabled} onChange={(e) => onToggle(e.target.checked)} /><span className="tk" /></span>
        </label>
      </div>
    </div>

    <div className="gw-default">
      <span>{t("请求里写")} <code>{card.id}</code> {t("时用")}</span>
      <Select value={card.defaultIsChosen ? (card.defaultModel ?? "") : ""} width={200}
        onChange={(v) => onDefault(v || null, card.defaultEffort ?? null)} options={modelOptions} />
      <Select value={card.defaultIsChosen ? (card.defaultEffort ?? "") : ""} width={150}
        onChange={(v) => onDefault(card.defaultModel ?? null, v || null)} options={effortOptions} />
      {!card.defaultIsChosen && <span className="s dim">{t("现在由 CLI 自己决定；选一个就固定下来")}</span>}
    </div>

    {open && <>
      <div className="gw-use">
        <div className="gw-use-tabs">
          {SNIPPETS.map(([k, label]) => <button key={k} className={k === kind ? "on" : ""} onClick={() => setKind(k)}>{t(label)}</button>)}
        </div>
        <pre className="gw-use-code">{snippet(kind, base, token, card.id)}</pre>
        <div className="gw-use-bar">
          <i className="ti ti-info-circle" /> {t("端口和密钥已经填好；要指定模型，把 model 换成下面表里的调用名")}
          <button className="pr sm" onClick={() => onCopy(snippet(kind, base, token, card.id))}><i className="ti ti-copy" /> {t("复制")}</button>
        </div>
      </div>

      <div className="gw-modelhd">
        <b>{t("可指定的模型")}</b>
        {card.models.length > SHOWN_MODELS && <label className="gw-search">
          <i className="ti ti-search" />
          <input value={query} placeholder={t("搜索模型…")} onChange={(e) => setQuery(e.target.value)} />
        </label>}
        {!!shared.length && <span className="s dim">{t("这些模型都支持")} {shared.join(" / ")} {t("推理档位；只有不一样的会在行内标注")}</span>}
      </div>

      <div className="gw-models">
        {shown.map((m) => {
          const note = effortNote(m.efforts, shared);
          const isDefault = card.defaultIsChosen && card.defaultModel === m.call.slice(card.id.length + 1);
          return <div className={"gw-model" + (isDefault ? " on" : "")} key={m.call}>
            <span className="nm">
              <b>{m.label} {isDefault && <span className="bd g">{t("当前默认")}</span>}{note && <span className="s dim"> · {t(note)}</span>}</b>
              <code>{m.call}</code>
            </span>
            <span className="acts">
              {testLine(m.call)}
              {!isDefault && <button className="gh xs" title={t("设为默认模型")} onClick={() => onDefault(m.call.slice(card.id.length + 1), card.defaultEffort ?? null)}><i className="ti ti-star" /></button>}
              <button className="gh xs" title={t("复制调用名")} onClick={() => onCopy(m.call)}><i className="ti ti-copy" /></button>
              <button className="gh xs" disabled={running(m.call)} title={t("测试这个模型")} onClick={() => onTest(m.call)}><i className="ti ti-player-play" /></button>
            </span>
          </div>;
        })}
        {!card.models.length && <p className="proxy-note">{t("没有读到模型列表，可在请求里直接写完整模型名。")}</p>}
        {!needle && matches.length > SHOWN_MODELS && <button className="gh sm gw-more" onClick={() => setAll(!all)}>
          {all ? t("收起") : `${t("展开其余")} ${matches.length - SHOWN_MODELS} ${t("个模型")}`}
        </button>}
        {!!needle && !matches.length && <p className="proxy-note">{t("没有匹配的模型。")}</p>}
      </div>
    </>}
  </div>;
}
