import { useEffect, useState } from "react";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { Select } from "../../Select";
import { useToast } from "../../ui";

export type AiView = {
  kind: "none" | "external";
  protocol: "openai" | "anthropic";
  baseUrl: string;
  model: string;
  hasKey: boolean;
  /** "" leaves it to the model; otherwise one of the levels the source takes. */
  effort: string;
};

const OPENAI_LEVELS = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];
const ANTHROPIC_LEVELS = ["low", "medium", "high", "xhigh", "max"];
function levelsFor(view: AiView): string[] {
  return view.protocol === "anthropic" ? ANTHROPIC_LEVELS : OPENAI_LEVELS;
}

export const AI_ERRORS: Record<string, string> = {
  E_AI_NONE: "还没有配置 AI：到「偏好设置 → AI 能力」填一个外部 API。",
  E_AI_FIELDS: "外部 API 的地址和模型都要填。",
  E_AI_KEY: "无法读取已保存的密钥，请重新填写。",
  E_AI_REPLY: "无法解析接口返回的内容，请确认接口类型是否正确。",
  E_AI_KIND: "无效的 AI 来源。",
  E_AI_PROTOCOL: "无效的接口类型。",
  E_AI_EFFORT: "无效的推理强度。",
  E_AI_NO_NOTES: "未获取到这次更新的官方日志，已跳过 AI 总结。可以到产品主页查看更新说明。",
  E_PROBE_URL: "请输入以 http:// 或 https:// 开头的地址。",
};
export const aiError = (e: unknown) => AI_ERRORS[String(e)] ?? String(e);

/**
 * Where Stacker's own AI features get a model from: an external API, paid per use with the
 * user's own key. Signed-in agents are not offered: several vendors forbid driving their
 * subscriptions from other programs.
 */
export function AiSettings() {
  const { tr } = useI18n();
  const toast = useToast();
  const [view, setView] = useState<AiView | null>(null);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [test, setTest] = useState<{ ok: boolean; text: string } | null>(null);
  // What is saved, so a test of unsaved edits saves them first.
  const [saved, setSaved] = useState<AiView | null>(null);

  useEffect(() => {
    invoke<AiView>("ai_config_get").then((got) => { setView(got); setSaved(got); }).catch(() => undefined);
  }, []);

  if (!view) return null;

  async function save(next: AiView, withKey = key, clearKey = false): Promise<boolean> {
    setBusy(true);
    setTest(null);
    try {
      // A level the source no longer takes (the protocol changed) is not sent.
      const effort = levelsFor(next).includes(next.effort) ? next.effort : "";
      const saved = await invoke<AiView>("ai_config_set", { update: { ...next, effort, apiKey: withKey, clearKey } });
      setView(saved);
      setSaved(saved);
      setKey("");
      toast(tr("AI 设置已保存"), "ok");
      return true;
    } catch (e) { toast(tr(aiError(e)), "err"); return false; }
    finally { setBusy(false); }
  }

  async function runTest() {
    // The test is of what is on screen: edits not yet saved are saved first.
    const dirty = key !== "" || JSON.stringify(view) !== JSON.stringify(saved);
    if (dirty && view && !(await save(view))) return;
    setBusy(true);
    setTest(null);
    try {
      const reply = await invoke<string>("ai_config_test");
      setTest({ ok: true, text: reply.trim().slice(0, 120) });
    } catch (e) { setTest({ ok: false, text: tr(aiError(e)) }); }
    finally { setBusy(false); }
  }

  const update = (patch: Partial<AiView>) => setView({ ...view, ...patch });
  // Each API's own values: OpenAI's reasoning_effort, Anthropic's output_config.effort.
  const levels = levelsFor(view);

  return <>
    <div className="srcrow">
      <span className="av st"><i className="ti ti-sparkles" /></span>
      <div className="mt">
        <div className="t">{tr("AI 来源")}</div>
        <div className="s dim" title={tr("各家智能体的会员订阅不允许被其他程序调用，所以这里不提供本机智能体。")}>{tr("Stacker 自己的 AI 功能（解释清理项、会话摘要等）用这里的模型，按你自己的 API Key 计费。")}</div>
      </div>
      <Select value={view.kind} width={170} onChange={(v) => update({ kind: v as AiView["kind"] })} options={[
        { value: "none", label: tr("不使用") },
        { value: "external", label: tr("外部 API") },
      ]} />
    </div>

    {view.kind !== "none" && <div className="srcrow">
      <span className="av st"><i className="ti ti-brain" /></span>
      <div className="mt">
        <div className="t">{tr("推理强度")}</div>
        <div className="s dim">{view.protocol === "anthropic"
            ? tr("作为 Anthropic 接口的 output_config.effort 发送；模型不支持的档位会报错，那就选“默认”。")
            : tr("作为 OpenAI 接口的 reasoning_effort 发送，仅推理模型支持，可用档位取决于模型；如报错请选择「默认」。")}</div>
      </div>
      <Select value={levels.includes(view.effort) ? view.effort : ""} width={170}
        onChange={(v) => update({ effort: v })}
        options={[{ value: "", label: tr("默认（不指定）") }, ...levels.map((level) => ({ value: level, label: level }))]} />
    </div>}

    {view.kind === "external" && <div className="ai-external">
      <label><span>{tr("接口类型")}</span>
        <Select value={view.protocol} width={170} onChange={(v) => update({ protocol: v as AiView["protocol"] })} options={[
          { value: "openai", label: tr("OpenAI 兼容") },
          { value: "anthropic", label: tr("Anthropic 兼容") },
        ]} />
      </label>
      <label><span>{tr("接口地址")}</span>
        <input className="ip full" value={view.baseUrl}
          placeholder={view.protocol === "anthropic" ? "https://api.anthropic.com" : "https://api.openai.com/v1"}
          onChange={(e) => update({ baseUrl: e.target.value })} /></label>
      <label><span>API Key</span>
        <input className="ip full" type="password" value={key} autoComplete="off"
          placeholder={view.hasKey ? tr("已保存；留空表示不改") : "sk-…"}
          onChange={(e) => setKey(e.target.value)} />
        {view.hasKey && <button className="gh sm" disabled={busy} onClick={() => void save(view, "", true)}>{tr("清除密钥")}</button>}
      </label>
      <label><span>{tr("模型")}</span>
        <input className="ip full" value={view.model} placeholder={view.protocol === "anthropic" ? "claude-sonnet-5" : "gpt-5.6"}
          onChange={(e) => update({ model: e.target.value })} /></label>
      <p className="s dim">{tr("密钥用你的 Windows 账户加密后单独存放，不进入配置备份；更换账户或设备后无法解密。")}</p>
    </div>}

    <div className="ai-actions">
      {test && <span className={"ai-test " + (test.ok ? "ok" : "bad")}>
        <i className={"ti " + (test.ok ? "ti-circle-check" : "ti-alert-circle")} /> {test.ok ? `${tr("已连通")}：${test.text}` : test.text}
      </span>}
      <button className="gh sm" disabled={busy || view.kind === "none"} onClick={() => void runTest()}>
        <i className={"ti " + (busy ? "ti-loader spin" : "ti-player-play")} /> {tr("测试")}
      </button>
      <button className="pr sm" disabled={busy} onClick={() => void save(view)}>
        <i className="ti ti-device-floppy" /> {tr("保存")}
      </button>
    </div>
  </>;
}
