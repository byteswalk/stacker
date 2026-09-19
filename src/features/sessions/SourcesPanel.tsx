import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { getRoots, getSummarySettings, openSession, runnerOptions, saveSummarySettings, setRoots } from "./api";
import { cleanSettings, RunnerFields } from "./RunnerFields";
import { errorMessage, type AgentOptions, type Roots, type RootsView, type SummarySettings } from "./types";

const ROWS: { key: keyof Roots; label: string; hint: string }[] = [
  { key: "codex", label: "Codex 数据目录", hint: "默认读取 CODEX_HOME，未设置时为用户目录下的 .codex" },
  { key: "claude", label: "Claude 数据目录", hint: "默认读取 CLAUDE_CONFIG_DIR，未设置时为用户目录下的 .claude" },
  { key: "claudeDesktopIndex", label: "Claude 桌面端会话索引", hint: "桌面端侧栏的会话列表，用于标题、归档和孤儿识别" },
];

export function SourcesPanel({ onSaved }: { onSaved: () => void }) {
  const { tr: t } = useI18n();
  const [view, setView] = useState<RootsView | null>(null);
  const [draft, setDraft] = useState<Roots | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const [summary, setSummary] = useState<SummarySettings | null>(null);
  const [savedSummary, setSavedSummary] = useState("");
  const [options, setOptions] = useState<AgentOptions[]>([]);

  useEffect(() => {
    Promise.all([getSummarySettings(), runnerOptions()])
      .then(([s, o]) => { setSummary(s); setSavedSummary(JSON.stringify(s)); setOptions(o); })
      .catch((e) => setError(errorMessage(e)));
  }, []);

  async function saveSummary() {
    if (!summary) return;
    setBusy(true); setError("");
    try { const clean = cleanSettings(summary); await saveSummarySettings(clean); setSummary(clean); setSavedSummary(JSON.stringify(clean)); onSaved(); }
    catch (e) { setError(errorMessage(e)); }
    finally { setBusy(false); }
  }

  useEffect(() => {
    getRoots().then((v) => { setView(v); setDraft(v.overrides); }).catch((e) => setError(errorMessage(e)));
  }, []);

  async function choose(key: keyof Roots) {
    const path = await open({ directory: true, multiple: false });
    if (typeof path === "string" && draft) setDraft({ ...draft, [key]: path });
  }
  async function save() {
    if (!draft) return;
    setBusy(true); setError("");
    try { const v = await setRoots(draft); setView(v); setDraft(v.overrides); onSaved(); }
    catch (e) { setError(errorMessage(e)); }
    finally { setBusy(false); }
  }

  if (!view || !draft) return error ? <p role="alert" className="session-error">{t(error)}</p> : null;
  const dirty = ROWS.some(({ key }) => draft[key] !== view.overrides[key]);
  return <div className="session-sources">
    {ROWS.map(({ key, label, hint }) => <div className="session-source" key={key}>
      <div className="session-source-head"><b>{t(label)}</b><small>{t(hint)}</small></div>
      <code title={view.effective[key]}>{view.effective[key]}</code>
      <div className="session-source-edit">
        <input className="ip full" value={draft[key]} placeholder={t("使用默认位置")} aria-label={t(label)} onChange={(e) => setDraft({ ...draft, [key]: e.target.value })} />
        <button className="gh sm" onClick={() => void choose(key).catch((e) => setError(errorMessage(e)))}><i className="ti ti-folder" />{t("选择…")}</button>
        <button className="gh sm" disabled={!draft[key]} onClick={() => setDraft({ ...draft, [key]: "" })}>{t("恢复默认")}</button>
      </div>
    </div>)}
    <div className="session-source">
      <div className="session-source-head"><b>{t("精简导出目录")}</b><small>{t("删除前导出的 Markdown 保存在这里")}</small></div>
      <div className="session-source-edit"><code title={view.exportDir}>{view.exportDir}</code><button className="gh sm" onClick={() => void openSession("", "exports").catch((e) => setError(errorMessage(e)))}><i className="ti ti-folder-open" />{t("打开")}</button></div>
    </div>
    <p className="session-note">{t("Stacker 只读取这些目录；删除会话时仅处理对应智能体目录内的文件。修改路径不会迁移数据。")}</p>
    {error && <p role="alert" className="session-error">{t(error)}</p>}
    <div><button className="pr sm" disabled={busy || !dirty} onClick={() => void save()}><i className="ti ti-device-floppy" />{t("保存")}</button></div>
    {summary && <div className="session-source summary-settings">
      <div className="session-source-head"><b>{t("摘要")}</b><small>{t("生成会话摘要和交接资料时使用的本机智能体、模型与推理强度。留空表示使用该命令行自己的默认值。")}</small></div>
      <RunnerFields value={summary} options={options} onChange={setSummary} />
      <div><button className="pr sm" disabled={busy || JSON.stringify(summary) === savedSummary} onClick={() => void saveSummary()}><i className="ti ti-device-floppy" />{t("保存摘要设置")}</button></div>
    </div>}
  </div>;
}
