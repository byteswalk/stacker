import { useCallback, useEffect, useState, type ReactNode } from "react";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { Modal, useBusy, useToast } from "../../ui";
import { FoldToggle, useFold } from "./Fold";
import { shortVersion } from "../agents/tileState";

type LoginStatus = { state: "logged_in" | "logged_out" | "unknown"; method: string };
type AgentModel = { call: string; label: string; efforts: string[]; defaultEffort: string | null };
export type AgentCard = {
  id: string; name: string; vendor: string; installed: boolean; version: string | null;
  supported: boolean; reason: string; login: LoginStatus | null; enabled: boolean;
  /** Reasoning levels a request naming only the agent takes. */
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
  E_RUNNER_START: "命令行程序启动失败，详情见日志。",
  E_RUNNER_NO_PLAN: "账号已登录，但没有开通命令行可用的套餐或额度已用完：到厂商那里开通编程套餐或充值后再试。桌面版的免费对话额度不能给命令行用。",
  E_RUNNER_TIMEOUT: "5 分钟内没有回复。",
  E_PROMPT_TOO_LONG: "内容太长：这个智能体只能从命令行接收提问，上限约 3 万字。",
};

const CACHE_KEY = "stacker.gateway.agents.v1";
const TESTS_KEY = "stacker.gateway.tests.v1";
/** A search box once the list is long enough to need one; every model is always listed. */
const SEARCH_FROM = 6;

type PastTest = { at: number; ok: boolean; elapsedMs: number };

function readJson<T>(key: string): T | null {
  try { return JSON.parse(localStorage.getItem(key) || "null") as T | null; } catch { return null; }
}

function writeJson(key: string, value: unknown) {
  try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* per-viewer convenience only */ }
}

/** The link a CLI printed with its refusal, for the button that opens it. */
export function detailLink(detail: string): string | null {
  return detail.split(/\s+/).find((word) => word.startsWith("https://")) ?? null;
}

/** The same message without the link, which the button carries instead. */
export function detailText(detail: string): string {
  const link = detailLink(detail);
  return (link ? detail.replace(link, "") : detail).trim();
}

/**
 * Whether the agent is signed in, said the same way for every agent. CodeBuddy cannot report
 * a sign-in, but it lists models only for a signed-in account, and a passing test proves one.
 */
export function agentState(card: AgentCard, past?: PastTest): { label: string; cls: string } {
  if (card.login?.state === "logged_out") return { label: "未登录", cls: "y" };
  if (card.login?.state === "logged_in" || past?.ok || card.models.length > 0) return { label: "已登录", cls: "g" };
  return { label: "未测试", cls: "n" };
}

/** One block per agent: can it be used, and each name it answers to with the levels that name takes. */
export function GatewayAgents({ base, token }: { base: string; token: string }) {
  const { tr: t } = useI18n();
  const toast = useToast();
  const busy = useBusy();
  const [cards, setCards] = useState<AgentCard[] | null>(() => readJson<AgentCard[]>(CACHE_KEY));
  const [refreshing, setRefreshing] = useState(false);
  const [tests, setTests] = useState<Record<string, TestResult | "running">>({});
  const [past, setPast] = useState<Record<string, PastTest>>(() => readJson<Record<string, PastTest>>(TESTS_KEY) ?? {});
  const [stackerAi, setStackerAi] = useState<StackerAi | null>(null);
  useEffect(() => {
    invoke<{ kind: string; localModel: string; effort: string }>("ai_config_get")
      .then((view) => setStackerAi(view.kind === "local" ? { call: view.localModel, effort: view.effort } : null))
      .catch(() => setStackerAi(null));
  }, []);

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

  /** A test of one name, at the level picked in its row; `call` is the agent's own id for the bare name. */
  async function test(card: AgentCard, call: string, effort: string | null) {
    const key = call;
    setTests((old) => ({ ...old, [key]: "running" }));
    try {
      const model = call === card.id ? null : call.slice(card.id.length + 1);
      const result = await busy({ title: `${t("正在测试")} ${call}${effort ? ` · ${effort}` : ""}`, message: t("发送一条简短消息并等待回复，通常需要 5–30 秒。") },
        () => invoke<TestResult>("gateway_test", { agent: card.id, model, effort }));
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
      onTest={(call, effort) => void test(card, call, effort)} onToggle={(on) => void toggle(card, on)} onCopy={copy}
      stackerAi={stackerAi} onStackerAi={setStackerAi} />)}

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
/** The name, and level, Stacker's own AI features run on, when that is a local agent. */
type StackerAi = { call: string; effort: string };

/**
 * Ready-to-run code for one name at one reasoning level, each in its own API's field:
 * `reasoning_effort` for OpenAI's Chat Completions, `output_config.effort` for Anthropic's
 * Messages. No level means none is sent and the agent decides.
 */
export function snippet(kind: Snippet, base: string, token: string, model: string, effort: string | null = null): string {
  if (kind === "openai") {
    const level = effort ? `\n    reasoning_effort="${effort}",` : "";
    return `from openai import OpenAI\n\nclient = OpenAI(\n    base_url="${base}/v1",\n    api_key="${token}",\n)\nreply = client.chat.completions.create(\n    model="${model}",${level}\n    messages=[{"role": "user", "content": "你好"}],\n)\nprint(reply.choices[0].message.content)`;
  }
  if (kind === "anthropic") {
    const level = effort ? `\n    output_config={"effort": "${effort}"},` : "";
    return `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="${base}",\n    api_key="${token}",\n)\nreply = client.messages.create(\n    model="${model}", max_tokens=1024,${level}\n    messages=[{"role": "user", "content": "你好"}],\n)\nprint(reply.content[0].text)`;
  }
  if (kind === "curl") {
    const level = effort ? `"reasoning_effort":"${effort}",` : "";
    return `curl ${base}/v1/chat/completions \\\n  -H "Authorization: Bearer ${token}" \\\n  -H "Content-Type: application/json" \\\n  -d '{"model":"${model}",${level}"messages":[{"role":"user","content":"你好"}]}'`;
  }
  const level = effort
    ? `推理强度：${effort}（客户端有“推理强度 / reasoning_effort”设置项就选它；没有这一项的客户端无法指定，由智能体自己决定）`
    : "推理强度：不指定，由智能体自己决定";
  return `接口类型：OpenAI 兼容（Chatbox、Cherry Studio、编辑器插件都选这个）\nAPI 地址：${base}/v1\nAPI 密钥：${token}\n模型名称：${model}\n${level}`;
}

const SNIPPETS: [Snippet, string][] = [
  ["openai", "OpenAI 兼容"],
  ["anthropic", "Anthropic 兼容"],
  ["curl", "curl"],
  ["client", "客户端填写"],
];

/** Every way to call one name at one level, ready to copy. */
function ExampleModal({ base, token, call, effort, onCopy, onClose }: {
  base: string; token: string; call: string; effort: string | null;
  onCopy: (text: string) => void; onClose: () => void;
}) {
  const { tr: t } = useI18n();
  const [kind, setKind] = useState<Snippet>("openai");
  const code = snippet(kind, base, token, call, effort);
  return <Modal wide icon="ti-code" title={<>{t("调用示例")} · <code>{call}</code> · {effort ?? t("跟随默认")}</>} onClose={onClose}
    footer={<button className="pr sm" onClick={() => onCopy(code)}><i className="ti ti-copy" /> {t("复制")}</button>}>
    <div className="gw-use-tabs">
      {SNIPPETS.map(([k, label]) => <button key={k} className={k === kind ? "on" : ""} onClick={() => setKind(k)}>{t(label)}</button>)}
    </div>
    <pre className="gw-use-code">{code}</pre>
  </Modal>;
}

/** Qoder lists only the models the account can use now; its own menu shows the rest. */
const PARTIAL_LIST: Record<string, string> = {
  qoder: "只列出这个账号现在能用的模型：额度用完时 Qoder 会停用付费模型，它们只在 Qoder 自己的菜单里显示，调用会失败；充值后自动出现在这里。",
  qodercn: "只列出这个账号现在能用的模型：额度用完时 Qoder 会停用付费模型，它们只在 Qoder 自己的菜单里显示，调用会失败；充值后自动出现在这里。",
};

function AgentBlock({ card, base, token, past, testLine, running, onTest, onToggle, onCopy, stackerAi, onStackerAi }: {
  card: AgentCard;
  base: string;
  token: string;
  past: Record<string, PastTest>;
  testLine: (key: string) => ReactNode;
  running: (key: string) => boolean;
  onTest: (call: string, effort: string | null) => void;
  onToggle: (enabled: boolean) => void;
  onCopy: (text: string) => void;
  stackerAi: StackerAi | null;
  onStackerAi: (next: StackerAi) => void;
}) {
  const toast = useToast();
  const { tr: t } = useI18n();
  const [open, toggleOpen] = useFold(`agent:${card.id}`, false);
  const [query, setQuery] = useState("");
  // The level picked in each row, for that row's test, examples and Stacker AI.
  const [levels, setLevels] = useState<Record<string, string>>({});
  const [example, setExample] = useState<string | null>(null);
  const pick = (call: string) => levels[call] ?? null;
  // One click makes this name, at this level, what Stacker's own AI features use.
  async function adoptForStacker(call: string) {
    try {
      await invoke("ai_config_use_local", { model: call, effort: pick(call) });
      onStackerAi({ call, effort: pick(call) ?? "" });
      toast(`${t("Stacker 的 AI 已改用")} ${call}${pick(call) ? ` · ${pick(call)}` : ""}`, "ok");
    } catch (e) { toast(String(e), "err"); }
  }
  const state = agentState(card, past[card.id]);
  const method = card.login?.state === "logged_in" && card.login.method ? card.login.method : "";
  const subtitle = [card.vendor, method, `${card.models.length} ${t("个模型")}`].filter(Boolean).join(" · ");
  const needle = query.trim().toLowerCase();
  // The bare name first: what the agent runs when a request names no model.
  const rows = [
    { call: card.id, label: t("跟随 CLI 默认"), efforts: card.efforts },
    ...card.models.map((m) => ({ call: m.call, label: m.label, efforts: m.efforts })),
  ].filter((m) => !needle || m.label.toLowerCase().includes(needle) || m.call.toLowerCase().includes(needle));

  return <div className={"pxcard gw-agent" + (open ? " open" : "")}>
    <div className="gw-head">
      <FoldToggle open={open} onToggle={toggleOpen} label={t(open ? "收起" : "展开")} />
      <div className="gw-title" onClick={toggleOpen}>
        <b>{card.name} {card.version && <span className="s dim">v{shortVersion(card.version)}</span>}</b>
        <span className="s dim">{subtitle}</span>
      </div>
      <span className={"bd " + state.cls}>{t(state.label)}</span>
      <div className="gw-agent-actions">
        <label className="gw-switch" title={t("关闭后接口服务不再接受这个智能体的请求")}>
          <span>{t(card.enabled ? "已开放" : "已关闭")}</span>
          <span className="sw"><input type="checkbox" checked={card.enabled} onChange={(e) => onToggle(e.target.checked)} /><span className="tk" /></span>
        </label>
      </div>
    </div>

    {open && <div className="gw-modelbox">
      {(card.models.length >= SEARCH_FROM || PARTIAL_LIST[card.id]) && <div className="gw-modelhd">
        {PARTIAL_LIST[card.id] && <span className="s dim"><i className="ti ti-info-circle" /> {t(PARTIAL_LIST[card.id])}</span>}
        {card.models.length >= SEARCH_FROM && <label className="gw-search">
          <i className="ti ti-search" />
          <input value={query} placeholder={t("搜索模型…")} onChange={(e) => setQuery(e.target.value)} />
        </label>}
      </div>}
      <div className="gw-models">
        {rows.map((m) => {
          const inUse = stackerAi?.call === m.call;
          return <div className={"gw-model" + (inUse ? " ai-on" : "")} key={m.call}>
          <span className="nm">
            <b>{m.label} <code>{m.call}</code>
              {inUse && <span className="gw-ai-badge"><i className="ti ti-sparkles" /> {t("Stacker AI 在用")}{stackerAi?.effort ? ` · ${stackerAi.effort}` : ""}</span>}</b>
            <span className="gw-levels">
              {m.efforts.length
                ? m.efforts.map((e) => <button key={e} className={"gw-level" + (pick(m.call) === e ? " on" : "")}
                  aria-pressed={pick(m.call) === e}
                  onClick={() => setLevels((old) => ({ ...old, [m.call]: old[m.call] === e ? "" : e }))}>{e}</button>)
                : <span className="s dim">{t("由 CLI 自己的设置决定")}</span>}
            </span>
          </span>
          <span className="acts">
            {testLine(m.call)}
            <button className="gh sm" disabled={running(m.call)} onClick={() => onTest(m.call, pick(m.call) || null)}><i className="ti ti-player-play" /> {t("测试")}</button>
            <button className="gh sm" onClick={() => setExample(m.call)}><i className="ti ti-code" /> {t("调用示例")}</button>
            <button className={"gh sm gw-ai-btn" + (inUse ? " on" : "")} aria-pressed={inUse}
              title={t(inUse ? "Stacker 自己的 AI 正在用这一行；再点一次会按这一行现在选的推理强度更新" : "设为 Stacker 自己的 AI（同步到偏好设置）")}
              aria-label={t("设为 Stacker 自己的 AI")} onClick={() => void adoptForStacker(m.call)}><i className="ti ti-sparkles" /></button>
          </span>
        </div>;
        })}
        {!!needle && !rows.length && <p className="proxy-note">{t("没有匹配的模型。")}</p>}
      </div>
      <p className="s dim gw-levels-note">{t("推理强度只用于这一行的测试、调用示例和设为 Stacker AI，不选就跟随默认。客户端请求时用接口的标准字段指定：OpenAI 兼容用 reasoning_effort，Anthropic 兼容用 output_config.effort。")}</p>
    </div>}
    {example && <ExampleModal base={base} token={token} call={example} effort={pick(example) || null}
      onCopy={onCopy} onClose={() => setExample(null)} />}
  </div>;
}
