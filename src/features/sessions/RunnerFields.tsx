import { useI18n } from "../../i18n";
import { Select } from "../../Select";
import type { AgentName, AgentOptions, SummarySettings } from "./types";

const CUSTOM = "__custom__";

type AgentKeys = { model: "codexModel" | "claudeModel"; effort: "codexEffort" | "claudeEffort" };
const KEYS: Record<AgentName, AgentKeys> = {
  codex: { model: "codexModel", effort: "codexEffort" },
  claude: { model: "claudeModel", effort: "claudeEffort" },
};
const NAME: Record<AgentName, string> = { codex: "Codex", claude: "Claude" };

/** Runner, model and reasoning effort for summaries; used for saved defaults and one-off overrides. */
export function RunnerFields({ value, options, onChange, agents }: {
  value: SummarySettings;
  options: AgentOptions[];
  onChange: (next: SummarySettings) => void;
  /** Agents whose model/effort rows are shown; defaults to both. */
  agents?: AgentName[];
}) {
  const { tr: t } = useI18n();
  const shown = agents ?? ["codex", "claude"];
  return <div className="runner-fields">
    <label className="runner-field">
      <span>{t("执行者")}</span>
      <Select value={value.runner} onChange={(runner) => onChange({ ...value, runner: runner as SummarySettings["runner"] })} options={[
        { value: "same", label: t("同源（Codex 会话用 Codex，Claude 会话用 Claude）") },
        { value: "codex", label: t("固定使用 Codex") },
        { value: "claude", label: t("固定使用 Claude") },
      ]} />
    </label>
    {shown.map((agent) => {
      const opt = options.find((o) => o.agent === agent);
      const keys = KEYS[agent];
      const model = value[keys.model];
      const known = opt?.models.find((m) => m.id === model);
      const custom = !!model && !known;
      const efforts = known?.efforts.length ? known.efforts : opt?.efforts ?? [];
      return <div key={agent} className="runner-agent">
        <b>{NAME[agent]}{opt && !opt.installed && <em>{t("未安装")}</em>}</b>
        <label className="runner-field">
          <span>{t("模型")}</span>
          <Select value={custom ? CUSTOM : model} onChange={(next) => onChange({ ...value, [keys.model]: next === CUSTOM ? " " : next })} options={[
            { value: "", label: t("命令行默认") },
            ...(opt?.models ?? []).map((m) => ({ value: m.id, label: m.label === m.id ? m.id : `${m.label} (${m.id})` })),
            { value: CUSTOM, label: t("自定义…") },
          ]} />
          {custom && <input className="ip" aria-label={`${NAME[agent]} ${t("模型名称")}`} value={model.trim()} placeholder={t("完整模型名称")} onChange={(e) => onChange({ ...value, [keys.model]: e.target.value || " " })} />}
        </label>
        <label className="runner-field">
          <span>{t("推理强度")}</span>
          <Select value={value[keys.effort]} onChange={(effort) => onChange({ ...value, [keys.effort]: effort })} options={[
            { value: "", label: t("命令行默认") },
            ...efforts.map((e) => ({ value: e, label: e })),
          ]} />
        </label>
      </div>;
    })}
  </div>;
}

/** The trimmed settings actually sent to the backend. */
export function cleanSettings(s: SummarySettings): SummarySettings {
  return { ...s, codexModel: s.codexModel.trim(), claudeModel: s.claudeModel.trim() };
}
